import { fileURLToPath } from "node:url";

export function webConfig(root, bindings) {
    const headers = {
        "Cross-Origin-Opener-Policy": "same-origin",
        "Cross-Origin-Embedder-Policy": "require-corp",
    };
    return {
        root,
        resolve: { alias: {
            "web-thread:wasm-shim": fileURLToPath(new URL("./worker-shim.mjs", import.meta.url)),
            "relocade:wasm-bindings": bindings,
        } },
        worker: { format: "es" },
        server: { headers },
        preview: { headers },
    };
}
