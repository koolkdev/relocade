import { fileURLToPath } from "node:url";
import { buildBindings, cargo as buildCargo } from "../../tools/web/build.mjs";

export function cargo(command, args) {
    buildCargo(command, args, {
        runner: `node ${fileURLToPath(new URL("./wasm-test.mjs", import.meta.url))}`,
    });
}

export function build() {
    buildBindings("relocade-browser-tests", fileURLToPath(new URL("./pkg", import.meta.url)));
}

if (import.meta.main) build();
