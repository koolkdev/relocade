import { cargo } from "./build.mjs";

export function testBackends(filters = []) {
    cargo("test", [
        "-p", "relocade-wasm-bindings", "-p", "relocade-wasm-runtime", "-p", "relocade-machine", "-p", "relocade-bios", "-p", "relocade-renderer", "-p", "relocade-vga", "-p", "relocade-app", "-p", "relocade-file-picker",
        "--features", "relocade-machine/worker,relocade-vga/renderer,relocade-app/graphics",
        ...(filters.length ? filters : ["--tests"]),
    ]);
}

if (import.meta.main) testBackends(process.argv.slice(2));
