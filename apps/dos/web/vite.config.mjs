import { fileURLToPath } from "node:url";

export default {
    root: fileURLToPath(new URL(".", import.meta.url)),
    resolve: { alias: {
        "web-thread:wasm-shim": fileURLToPath(new URL("../../../tools/web/worker-shim.mjs", import.meta.url)),
        "relocade:wasm-bindings": fileURLToPath(new URL("./pkg/relocade_dos_app.js", import.meta.url)),
    } },
    server: {
        fs: { allow: [fileURLToPath(new URL("../../..", import.meta.url))] },
        headers: {
            "Cross-Origin-Opener-Policy": "same-origin",
            "Cross-Origin-Embedder-Policy": "require-corp",
        },
    },
};
