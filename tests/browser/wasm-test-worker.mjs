import { prepareTests } from "./wasm-test-suite.mjs";

// wasm-bindgen-test's worker formatter writes one line at a time.
globalThis.__wbg_test_output_writeln = line => postMessage({ line });
self.onmessage = async ({ data }) => {
    try {
        const run = await prepareTests();
        await run(data);
        postMessage({ done: true });
    } catch (error) {
        postMessage({ error: error?.stack ?? String(error) });
    }
};
