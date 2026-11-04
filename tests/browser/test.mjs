import { build } from "./build.mjs";
import { withBrowser } from "./browser-test.mjs";
import { testBackends } from "./test-backends.mjs";
import { testApplication } from "./dos-app.mjs";
import { configure } from "../../tools/web.mjs";

await build();
await withBrowser(async browser => {
    await testBackends(browser, process.argv.slice(2));
    await browser.run();
});
await testApplication(await configure("relocade-dos-app"));
