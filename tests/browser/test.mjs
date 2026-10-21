import { build, cargo } from "./build.mjs";
import { run } from "./browser-test.mjs";

// Backend tests and worker integration use the same threaded Rust build.
const filters = process.argv.slice(2);
cargo("test", [
    "-p", "relocade-wasm-runtime", "-p", "relocade-runtime", "-p", "relocade-renderer",
    ...(filters.length ? filters : ["--tests"]),
]);
build();
await run();
