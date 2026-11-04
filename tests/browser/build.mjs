import { fileURLToPath } from "node:url";
import path from "node:path";
import { cargo as webCargo, bindgen, target } from "../../tools/web-build.mjs";

const directory = fileURLToPath(new URL(".", import.meta.url));

// Resolve one feature graph for backend tests, worker fixtures and the application.
const packages = [
    "relocade-wasm-bindings", "relocade-wasm-runtime", "relocade-machine",
    "relocade-bios", "relocade-pit", "relocade-pc-speaker", "relocade-renderer",
    "relocade-vga", "relocade-app", "relocade-browser-tests", "relocade-dos-app",
];
const features = "relocade-vga/renderer,relocade-app/graphics,relocade-app/audio,relocade-pc-speaker/audio";

export function cargo(command, args) {
    webCargo(command, [
        ...packages.flatMap(name => ["-p", name]), "--features", features, ...args,
    ], `node ${path.join(directory, "wasm-test.mjs")}`);
}

export function build() {
    // Include dev dependencies now, so cargo test reuses the same libraries.
    cargo("build", ["--lib", "--tests", "--profile", "test"]);
    bindgen(path.join(target, "wasm32-unknown-unknown/debug/relocade_browser_tests.wasm"),
        path.join(directory, "pkg"));
}

if (import.meta.main) build();
