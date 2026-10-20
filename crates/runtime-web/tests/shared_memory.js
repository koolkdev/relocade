export async function write_in_worker(memory) {
    const source = `
        onmessage = ({ data: memory }) => {
            const original = new Uint8Array(memory.buffer)[0];
            memory.grow(1);
            new Uint8Array(memory.buffer).set([1, 2, 3, 4], 65536);
            postMessage(original);
        };
    `;
    const url = URL.createObjectURL(new Blob([source], { type: "text/javascript" }));
    const worker = new Worker(url);
    try {
        return await new Promise((resolve, reject) => {
            worker.onmessage = ({ data }) => resolve(data);
            worker.onerror = reject;
            worker.postMessage(memory);
        });
    } finally {
        worker.terminate();
        URL.revokeObjectURL(url);
    }
}
