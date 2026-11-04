import { cargo } from "./build.mjs";

export function testBackends(filters = []) {
    cargo("test", [
        "-p", "relocade-wasm-bindings", "-p", "relocade-wasm-runtime", "-p", "relocade-machine", "-p", "relocade-bios", "-p", "relocade-pit", "-p", "relocade-pc-speaker", "-p", "relocade-renderer", "-p", "relocade-vga", "-p", "relocade-app",
        "--features", "relocade-vga/renderer,relocade-app/graphics,relocade-app/audio,relocade-pc-speaker/audio",
        ...(filters.length ? filters : ["--tests"]),
    ]);
}

if (import.meta.main) testBackends(process.argv.slice(2));
