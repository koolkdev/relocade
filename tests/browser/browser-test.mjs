import { fileURLToPath } from "node:url";
import { withBrowser } from "../../tools/web/browser.mjs";

export async function run(url = "/", bindings) {
    await withBrowser({
        configFile: fileURLToPath(new URL("./vite.config.mjs", import.meta.url)),
        resolve: bindings ? { alias: { "relocade:wasm-bindings": bindings } } : undefined,
    }, async (page, address) => {
        try {
            await page.goto(new URL(url, address).href);
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
