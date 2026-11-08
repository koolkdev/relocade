import assert from "node:assert/strict";
import { test } from "node:test";
import { DeadlineTimer } from "../../crates/machine/src/clock/driver/wait/browser/timer.js";

// Control elapsed message-queue time without relying on host timer resolution.
function scheduler(t) {
    let now = 100;
    const messages = [];
    const timeouts = new Map();
    let nextTimeout = 0;
    t.mock.method(performance, "now", () => now);
    t.mock.method(globalThis, "MessageChannel", function () {
        this.port1 = { onmessage: null, close() {} };
        this.port2 = {
            postMessage: data => messages.push(() => this.port1.onmessage?.({ data })),
            close() {},
        };
    });
    t.mock.method(globalThis, "setTimeout", (callback, delay) => {
        const id = ++nextTimeout;
        timeouts.set(id, { callback, delay });
        return id;
    });
    t.mock.method(globalThis, "clearTimeout", id => timeouts.delete(id));
    return {
        timeouts,
        messageAfter(milliseconds) {
            now += milliseconds;
            messages.shift()();
        },
    };
}

test("future deadlines round upward after accounting for message queue time", async t => {
    const host = scheduler(t);
    const timer = new DeadlineTimer();
    t.after(() => timer.close());
    for (const [duration, queued, expected] of [
        [0.484, 0, 1],
        [2.75, 1.5, 2],
        [2, 0, 2],
        [3_000_000_000, 0, 0x7fffffff],
    ]) {
        const wait = timer.start(duration);
        host.messageAfter(queued);
        assert.equal(host.timeouts.size, 1);
        const [id, timeout] = host.timeouts.entries().next().value;
        assert.equal(timeout.delay, expected);
        host.timeouts.delete(id);
        timeout.callback();
        await wait;
    }
});

test("expired and cancelled queued deadlines need no timeout", async t => {
    const host = scheduler(t);
    const timer = new DeadlineTimer();
    t.after(() => timer.close());
    for (const [duration, queued] of [[0, 0], [0.5, 1]]) {
        let completed = false;
        const wait = timer.start(duration).then(() => { completed = true; });
        host.messageAfter(queued);
        await Promise.resolve();
        assert.equal(completed, true);
        assert.equal(host.timeouts.size, 0);
        await wait;
    }
    const cancelled = timer.start(1);
    timer.cancel();
    const replacement = timer.start(0.5);
    host.messageAfter(0); // Ignore the cancelled request's queued message.
    assert.equal(host.timeouts.size, 0);
    host.messageAfter(1);
    await Promise.all([cancelled, replacement]);
    assert.equal(host.timeouts.size, 0);
});
