import { build } from "./build.mjs";
import { run } from "./browser-test.mjs";
import { testBackends } from "./test-backends.mjs";
import { testApplication } from "./dos-app.mjs";
import { configure } from "../../tools/web.mjs";

build();
testBackends(process.argv.slice(2));
await run();
await testApplication(configure("relocade-dos-app"));
