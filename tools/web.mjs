import { fileURLToPath } from "node:url";
import path from "node:path";
import { gzipSync } from "node:zlib";
import { build as bundle, createServer } from "vite";
import { cargo, bindgen, target } from "./web-build.mjs";

const root = fileURLToPath(new URL("..", import.meta.url));
const isolationHeaders = {
    "Cross-Origin-Opener-Policy": "same-origin",
    "Cross-Origin-Embedder-Policy": "require-corp",
};
const wasmHeaders = { "Content-Encoding": "gzip", "Content-Type": "application/wasm" };

function compressedWasm() {
    return {
        name: "compressed-wasm",
        generateBundle(_options, bundle) {
            const headers = ["/*", ...Object.entries(isolationHeaders).map(([key, value]) => `  ${key}: ${value}`)];
            for (const asset of Object.values(bundle)) {
                if (asset.type !== "asset" || !asset.fileName.endsWith(".wasm")) continue;
                asset.source = gzipSync(asset.source, { level: 9 });
                headers.push("", `/${asset.fileName}`, ...Object.entries(wasmHeaders).map(([key, value]) => `  ${key}: ${value}`));
            }
            this.emitFile({ type: "asset", fileName: "_headers", source: `${headers.join("\n")}\n` });
        },
        configurePreviewServer(server) {
            server.middlewares.use((request, response, next) => {
                if (request.url?.split("?")[0].endsWith(".wasm")) {
                    for (const [key, value] of Object.entries(wasmHeaders)) response.setHeader(key, value);
                }
                next();
            });
        },
    };
}

export function build(application) {
    if (!/^[a-z][a-z0-9-]*$/.test(application)) throw new Error("Expected a Cargo package name");
    cargo("build", ["-p", application, "--lib", "--profile", "test"]);
    const output = path.join(root, "target/web", application);
    const module = application.replaceAll("-", "_");
    bindgen(path.join(target, `wasm32-unknown-unknown/debug/${module}.wasm`), path.join(output, "pkg"));
    return {
        configFile: false,
        plugins: [compressedWasm()],
        root: path.join(root, "crates/app/web"),
        resolve: { alias: {
            "web-thread:wasm-shim": path.join(root, "tools/worker-shim.mjs"),
            "relocade:wasm-bindings": path.join(output, `pkg/${module}.js`),
        } },
        server: {
            host: "127.0.0.1",
            fs: { allow: [root] },
            headers: isolationHeaders,
        },
        build: { outDir: path.join(output, "dist"), emptyOutDir: true },
    };
}

if (import.meta.main) {
    const [, , application, mode] = process.argv;
    if (!application || (mode && mode !== "--build")) throw new Error("Usage: node tools/web.mjs package-name [--build]");
    const config = build(application);
    if (mode === "--build") {
        await bundle(config);
    } else {
        const server = await createServer(config);
        await server.listen();
        server.printUrls();
    }
}
