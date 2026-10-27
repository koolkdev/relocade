import { fileURLToPath } from "node:url";

const isolationHeaders = {
    "Cross-Origin-Opener-Policy": "same-origin",
    "Cross-Origin-Embedder-Policy": "require-corp",
};

export function webConfig(root, bindings) {
    return {
        root,
        resolve: {
            alias: {
                "web-thread:wasm-shim": fileURLToPath(new URL("./worker-shim.mjs", import.meta.url)),
                "relocade:wasm-bindings": bindings,
            },
        },
        worker: { format: "es" },
        server: { headers: isolationHeaders },
        preview: { headers: isolationHeaders },
    };
}
