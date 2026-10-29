import { cargo } from "./build.mjs";

export function testBackends(filters = []) {
    cargo("test", [
        "-p", "relocade-wasm-bindings", "-p", "relocade-wasm-runtime", "-p", "relocade-machine", "-p", "relocade-renderer", "-p", "relocade-vga",
        "--features", "relocade-vga/renderer",
        ...(filters.length ? filters : ["--tests"]),
    ]);
}

if (import.meta.main) testBackends(process.argv.slice(2));
