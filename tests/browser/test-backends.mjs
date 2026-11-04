import { cargo } from "./build.mjs";
import { withBrowser } from "./browser-test.mjs";

export function testBackends(browser, filters = []) {
    return cargo("test", filters.length ? filters : ["--tests"], {
        RELOCADE_BROWSER_RUNNER: browser.endpoint,
    });
}

if (import.meta.main) await withBrowser(browser => testBackends(browser, process.argv.slice(2)));
