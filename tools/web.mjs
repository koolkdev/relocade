import { fileURLToPath } from "node:url";
import path from "node:path";
import { build as bundle, createServer } from "vite";
import { cargo, bindgen, target } from "./web-build.mjs";

const root = fileURLToPath(new URL("..", import.meta.url));

export function build(application) {
    if (!/^[a-z][a-z0-9-]*$/.test(application)) throw new Error("Expected a Cargo package name");
    cargo("build", ["-p", application, "--lib", "--profile", "test"]);
    const output = path.join(root, "target/web", application);
    const module = application.replaceAll("-", "_");
    bindgen(path.join(target, `wasm32-unknown-unknown/debug/${module}.wasm`), path.join(output, "pkg"));
    return {
        configFile: false,
        root: path.join(root, "crates/app/web"),
        resolve: { alias: {
            "web-thread:wasm-shim": path.join(root, "tools/worker-shim.mjs"),
            "relocade:wasm-bindings": path.join(output, `pkg/${module}.js`),
        } },
        server: {
            host: "127.0.0.1",
            fs: { allow: [root] },
            headers: {
                "Cross-Origin-Opener-Policy": "same-origin",
                "Cross-Origin-Embedder-Policy": "require-corp",
            },
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
