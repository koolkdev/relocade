import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";

export default defineConfig({
    root: fileURLToPath(new URL(".", import.meta.url)),
    resolve: {
        alias: {
            "web-thread:wasm-shim": fileURLToPath(new URL("./worker-shim.mjs", import.meta.url)),
            "relocade:wasm-bindings": fileURLToPath(new URL("./pkg/relocade_browser_tests.js", import.meta.url)),
        },
    },
    server: {
        headers: {
            "Cross-Origin-Opener-Policy": "same-origin",
            "Cross-Origin-Embedder-Policy": "require-corp",
        },
    },
});
