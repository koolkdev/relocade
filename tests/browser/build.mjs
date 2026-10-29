import { fileURLToPath } from "node:url";
import { cargo as buildCargo, buildBindings } from "../../tools/web/build.mjs";

export function cargo(command, args) {
    buildCargo(command, args, {
        CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER: `node ${fileURLToPath(new URL("./wasm-test.mjs", import.meta.url))}`,
        WASM_BINDGEN_TEST_TIMEOUT: process.env.WASM_BINDGEN_TEST_TIMEOUT ?? "120",
    });
}

export function build() {
    buildBindings("relocade-browser-tests", fileURLToPath(new URL("./pkg", import.meta.url)));
}

if (import.meta.main) build();
