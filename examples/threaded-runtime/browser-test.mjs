import { createServer } from "vite";
import { chromium } from "playwright";
import { fileURLToPath } from "node:url";

const server = await createServer({
    configFile: fileURLToPath(new URL("./vite.config.mjs", import.meta.url)),
    server: { host: "127.0.0.1", port: 0 },
});
await server.listen();
let browser;
try {
    browser = await chromium.launch({ executablePath: process.env.CHROME_BINARY });
    const page = await browser.newPage();
    page.on("console", message => console.log(message.text()));
    page.on("pageerror", error => console.error(error));
    await page.goto(server.resolvedUrls.local[0]);
    await page.waitForFunction(() => window.checks !== undefined);
    await Promise.race([
        page.evaluate(() => window.checks),
        new Promise((_, reject) => setTimeout(() => reject(new Error("Thread checks timed out")), 120_000).unref()),
    ]);
    console.log(await page.locator("#result").textContent());
} finally {
    await browser?.close();
    await server.close();
}
