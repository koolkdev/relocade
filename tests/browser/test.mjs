import { build } from "./build.mjs";
import { run } from "./browser-test.mjs";
import { testBackends } from "./test-backends.mjs";

// Backend tests and worker integration use the same threaded Rust build.
testBackends(process.argv.slice(2));
build();
await run();
