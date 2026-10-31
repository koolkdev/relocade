import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";

export default defineConfig({
    root: fileURLToPath(new URL(".", import.meta.url)),
    resolve: {
        alias: {
            "web-thread:wasm-shim": fileURLToPath(new URL("../../tools/worker-shim.mjs", import.meta.url)),
            "relocade:wasm-bindings": fileURLToPath(new URL("./pkg/relocade_browser_tests.js", import.meta.url)),
        },
    },
    server: {
        fs: { allow: [fileURLToPath(new URL("../..", import.meta.url))] },
        headers: {
            "Cross-Origin-Opener-Policy": "same-origin",
            "Cross-Origin-Embedder-Policy": "require-corp",
        },
    },
});
