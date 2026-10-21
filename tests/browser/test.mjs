import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { build, cargo } from "./build.mjs";

// Select the same Chrome for both runners, with software WebGPU for renderer tests.
if (!process.env.WASM_BINDGEN_TEST_WEBDRIVER_JSON) {
    const directory = mkdtempSync(join(tmpdir(), "relocade-browser-"));
    process.on("exit", () => rmSync(directory, { recursive: true, force: true }));
    const config = join(directory, "webdriver.json");
    writeFileSync(config, JSON.stringify({
        "goog:chromeOptions": {
            binary: process.env.CHROME_BINARY,
            args: ["--enable-unsafe-webgpu", "--use-angle=swiftshader", "--enable-unsafe-swiftshader"],
        },
    }));
    process.env.WASM_BINDGEN_TEST_WEBDRIVER_JSON = config;
}

// Backend tests and worker integration use the same threaded Rust build.
const filters = process.argv.slice(2);
cargo("test", [
    "-p", "relocade-runtime-web", "-p", "relocade-renderer",
    ...(filters.length ? filters : ["--tests"]),
]);
build();
await import("./browser-test.mjs");
