// Notifications do not consume messages. Cancelling a Rust receive future
// therefore leaves the next message available to another receive.
class Inbox {
    constructor() {
        this.messages = [];
        this.closed = false;
        this.error = null;
        this.notification = null;
    }
    available() {
        if (this.messages.length || this.closed) return Promise.resolve();
        if (!this.notification) {
            this.notification = new Promise(resolve => { this.wake = resolve; });
        }
        return this.notification;
    }
    notify() {
        if (this.notification) this.wake();
        this.notification = null;
    }
    push(message) {
        this.messages.push(message);
        this.notify();
    }
    close(error = null) {
        this.closed = true;
        this.error = error;
        this.notify();
    }
    take() {
        if (this.messages.length) return [true, this.messages.shift()];
        if (this.error) throw this.error;
        return [false, null];
    }
}

export class WorkerOwner {
    constructor(bindings, wasm, entry, configuration) {
        this.inputOpen = true;
        this.finished = false;
        this.didStart = false;
        this.events = new Inbox();
        this.readiness = new Promise((resolve, reject) => {
            this.started = resolve; this.startFailed = reject;
        });
        this.completion = new Promise((resolve, reject) => {
            this.stopped = resolve; this.stopFailed = reject;
        });
        // Lifecycle errors are observed even before Rust polls either future.
        this.readiness.catch(() => {});
        this.completion.catch(() => {});
        const source = `import { run } from ${JSON.stringify(import.meta.url)}; run();`;
        const url = URL.createObjectURL(new Blob([source], {type: "text/javascript"}));
        try {
            this.worker = new Worker(url, {type: "module", name: entry});
        } finally {
            URL.revokeObjectURL(url);
        }
        this.worker.onmessage = ({data}) => {
            switch (data.kind) {
                case "ready": this.didStart = true; this.started(); break;
                case "event": this.events.push(data.value); break;
                case "stopped": this.finish(); break;
                case "failed": this.finish(new Error(data.error)); break;
            }
        };
        this.worker.onerror = event => {
            event.preventDefault();
            this.finish(new Error(event.message || "worker failed"));
        };
        this.worker.onmessageerror = () => this.finish(new Error("invalid worker message"));
        try {
            this.worker.postMessage({kind: "start", bindings: new URL(bindings).href,
                wasm: new URL(wasm).href, entry, configuration});
        } catch (error) {
            this.finish(error);
        }
    }
    send(value, transfer) {
        if (!this.inputOpen) throw new Error("worker input is closed");
        this.worker.postMessage({kind: "command", value}, transfer);
    }
    shutdown() {
        if (!this.inputOpen) return;
        this.inputOpen = false;
        this.worker.postMessage({kind: "close"});
    }
    finish(error = null) {
        if (this.finished) return;
        this.finished = true;
        this.inputOpen = false;
        if (!this.didStart) {
            error ??= new Error("worker stopped before reporting readiness");
            this.startFailed(error);
        }
        this.events.close(error);
        if (error) this.stopFailed(error); else this.stopped();
        this.worker.terminate();
        this.worker.onmessage = this.worker.onerror = this.worker.onmessageerror = null;
    }
}

// This function runs in the child worker. Install command delivery before
// awaiting module initialization so startup and early shutdown cannot lose input.
export async function run() {
    const input = new Inbox();
    let start;
    const startup = new Promise(resolve => { start = resolve; });
    onmessage = ({data}) => {
        switch (data.kind) {
            case "start": start(data); break;
            case "command": input.push(data.value); break;
            case "close": input.close(); break;
        }
    };
    onmessageerror = () => {
        postMessage({kind: "failed", error: "invalid command message"});
        close();
    };
    input.ready = () => postMessage({kind: "ready"});
    input.send = (value, transfer) => postMessage({kind: "event", value}, transfer);
    try {
        const {bindings, wasm, entry, configuration} = await startup;
        const api = await import(bindings);
        await api.default({module_or_path: wasm});
        await api[entry](api, input, configuration);
        postMessage({kind: "stopped"});
    } catch (error) {
        postMessage({kind: "failed", error: String(error)});
    } finally {
        close();
    }
}
