import { build, cargo } from "./build.mjs";
import { run } from "./browser-test.mjs";
import { testBackends } from "./test-backends.mjs";

// Backend tests and worker integration use the same threaded Rust build.
// DOS and BIOS services use the same borrowed machine context on the browser.
cargo("check", ["-p", "relocade-dos", "--lib"]);
testBackends(process.argv.slice(2));
build();
await run();
