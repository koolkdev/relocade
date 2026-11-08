// Every timeout is armed from a message task. Promise continuations alone still
// inherit timer nesting and eventually hit HTML's 4 ms minimum for nested timers.
export class DeadlineTimer {
    constructor() {
        this.channel = new MessageChannel();
        this.pending = null;
        this.nextId = 0;
        this.channel.port1.onmessage = ({ data: id }) => {
            const pending = this.pending;
            if (!pending || pending.id !== id) return;
            // Queueing the message must count toward the original deadline.
            const remaining = Math.max(0, pending.deadline - performance.now());
            const complete = () => {
                if (this.pending !== pending) return;
                this.pending = null;
                pending.resolve();
            };
            if (remaining === 0) {
                complete();
                return;
            }
            // The API truncates to integer milliseconds. Round up so a positive
            // remainder cannot become zero and repeatedly wake before the deadline.
            // Bound long waits to the timer API's signed 32-bit millisecond range.
            pending.timeout = setTimeout(complete, Math.min(Math.ceil(remaining), 0x7fffffff));
        };
    }

    start(milliseconds) {
        const deadline = performance.now() + milliseconds;
        return new Promise(resolve => {
            const pending = { id: ++this.nextId, deadline, resolve, timeout: null };
            this.pending = pending;
            this.channel.port2.postMessage(pending.id);
        });
    }

    cancel() {
        const pending = this.pending;
        if (!pending) return;
        this.pending = null;
        if (pending.timeout !== null) clearTimeout(pending.timeout);
        // Settle even cancelled promises: JsFuture's callbacks remain alive
        // until settlement, including after the Rust future has been dropped.
        pending.resolve();
    }

    close() {
        this.cancel();
        this.channel.port1.onmessage = null;
        this.channel.port1.close();
        this.channel.port2.close();
    }
}
