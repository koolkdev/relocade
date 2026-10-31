import { fileURLToPath } from "node:url";
import path from "node:path";
import { cargo as webCargo, bindgen, target } from "../../tools/web-build.mjs";

const directory = fileURLToPath(new URL(".", import.meta.url));

export function cargo(command, args) {
    webCargo(command, args, `node ${path.join(directory, "wasm-test.mjs")}`);
}

export function build() {
    cargo("build", ["-p", "relocade-browser-tests", "--profile", "test"]);
    bindgen(path.join(target, "wasm32-unknown-unknown/debug/relocade_browser_tests.wasm"),
        path.join(directory, "pkg"));
}

if (import.meta.main) build();
