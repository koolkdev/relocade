export function started(memory) {
    postMessage({ kind: "ready", memory });
}

export function waiting() {
    postMessage({ kind: "waiting" });
}

export async function run_in_worker() {
    const bindings = new URL("/wasm-bindgen-test", location.href).href;
    const wasm = new URL("/wasm-bindgen-test_bg.wasm", location.href).href;
    const source = `
        try {
            const api = await import(${JSON.stringify(bindings)});
            await api.default({module_or_path: ${JSON.stringify(wasm)}});
            onmessage = ({data}) => {
                if (data.kind === "read") api.request_read();
                if (data.kind === "input") api.provide_input(data.byte);
            };
            const result = await api.run_cpu();
            postMessage({kind: "done", result});
        } catch (error) {
            postMessage({kind: "error", error: String(error)});
        }
    `;
    const url = URL.createObjectURL(new Blob([source], {type: "text/javascript"}));
    const worker = new Worker(url, {type: "module", name: "relocade-test-cpu"});
    try {
        return await new Promise((resolve, reject) => {
            worker.onerror = event => reject(new Error(event.message));
            worker.onmessage = ({data}) => {
                if (data.kind === "error") reject(new Error(data.error));
                if (data.kind === "ready") {
                    if (new Uint8Array(data.memory.buffer)[0x400] !== 1) {
                        reject(new Error("CPU writes are missing from shared memory"));
                        return;
                    }
                    // This command must be handled while the CPU is busy, before
                    // it reaches any deferred host operation.
                    worker.postMessage({kind: "read"});
                }
                if (data.kind === "waiting") {
                    worker.postMessage({kind: "input", byte: 42});
                }
                if (data.kind === "done") resolve(data.result);
            };
        });
    } finally {
        worker.terminate();
        URL.revokeObjectURL(url);
    }
}
