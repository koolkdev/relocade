import { withBrowser } from "../../tools/web/browser.mjs";
import { webConfig } from "../../tools/web/vite.mjs";
import { fileURLToPath } from "node:url";

export async function run(url = "/", bindings = fileURLToPath(new URL("./pkg/relocade_browser_tests.js", import.meta.url))) {
    await withBrowser(webConfig(fileURLToPath(new URL(".", import.meta.url)), bindings), async (page, base) => {
        try {
            await page.goto(new URL(url, base).href);
            await page.waitForFunction(() => window.checks !== undefined);
            await Promise.race([
                page.evaluate(() => window.checks),
                new Promise((_, reject) => setTimeout(() => reject(new Error("Browser tests timed out")),
                    Number(process.env.WASM_BINDGEN_TEST_TIMEOUT ?? 120) * 1000).unref()),
            ]);
        } finally {
            const output = await page.locator("#output").textContent({ timeout: 1000 }).catch(() => null);
            if (output) console.log(output);
        }
    });
}

if (import.meta.main) await run();
