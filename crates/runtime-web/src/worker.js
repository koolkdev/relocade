// Each worker initializes a private compiler instance. Guest memories, imports,
// callbacks and function tables remain with the execution owner.
export class CompilerWorker {
    constructor(bindingsUrl, wasmUrl) {
        const bindings = new URL(bindingsUrl).href;
        const binary = new URL(wasmUrl).href;
        const source = `
            const ready = import(${JSON.stringify(bindings)}).then(async api => {
                await api.default({ module_or_path: ${JSON.stringify(binary)} });
                return api;
            });
            ready.catch(() => {});
            let previous = Promise.resolve();
            onmessage = ({ data: { id, bytes } }) => {
                previous = previous.then(async () => {
                    try {
                        const api = await ready;
                        const result = api.relocade_compile_snapshot(bytes);
                        if (result === null) {
                            postMessage({ id, result: null });
                            return;
                        }
                        const module = await WebAssembly.compile(result[0]);
                        const metadata = result[1];
                        postMessage({ id, result: [metadata, module] }, [metadata.buffer]);
                    } catch (error) {
                        postMessage({ id, error: String(error) });
                    }
                });
            };
        `;
        const url = URL.createObjectURL(new Blob([source], { type: "text/javascript" }));
        try {
            this.worker = new Worker(url, { type: "module", name: "relocade-compiler" });
        } finally {
            URL.revokeObjectURL(url);
        }
        this.nextId = 0;
        this.pending = new Map();
        this.failure = null;
        this.worker.onmessage = ({ data }) => {
            const pending = this.pending.get(data.id);
            if (!pending) return;
            this.pending.delete(data.id);
            if (data.error !== undefined) pending.reject(new Error(data.error));
            else pending.resolve(data.result);
        };
        this.worker.onerror = event => {
            event.preventDefault();
            this.fail(new Error(event.message || "compiler worker failed"));
        };
        this.worker.onmessageerror = () => this.fail(new Error("invalid compiler worker message"));
    }

    submit(bytes) {
        if (this.failure) throw this.failure;
        if (this.pending.size >= 2) return null;
        const id = this.nextId++;
        return new Promise((resolve, reject) => {
            this.pending.set(id, { resolve, reject });
            try {
                this.worker.postMessage({ id, bytes }, [bytes.buffer]);
            } catch (error) {
                this.pending.delete(id);
                reject(error);
            }
        });
    }

    fail(error) {
        this.failure = error;
        for (const pending of this.pending.values()) pending.reject(error);
        this.pending.clear();
    }

    close() {
        this.worker.terminate();
        this.fail(new Error("compiler worker closed"));
        this.worker.onmessage = null;
        this.worker.onerror = null;
        this.worker.onmessageerror = null;
    }
}
