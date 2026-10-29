import { createServer } from "vite";
import { chromium } from "playwright";
import { fileURLToPath } from "node:url";

export async function run(url = "/", bindings) {
    const server = await createServer({
        configFile: fileURLToPath(new URL("./vite.config.mjs", import.meta.url)),
        resolve: bindings ? { alias: { "relocade:wasm-bindings": bindings } } : undefined,
        server: { host: "127.0.0.1", port: 0 },
    });
    await server.listen();
    let browser;
    let page;
    try {
        browser = await chromium.launch({
            executablePath: process.env.CHROME_BINARY,
            // Use the same software GL backend for WebGPU and canvas composition.
            // Offscreen readback also works with Dawn's default adapter, but that
            // adapter cannot share canvas images with SwiftShader's GL compositor.
            args: ["--enable-unsafe-webgpu", "--use-angle=swiftshader", "--enable-unsafe-swiftshader",
                "--use-webgpu-adapter=opengles", "--js-flags=--expose-gc"],
        });
        page = await browser.newPage();
        page.on("console", message => console.log(message.text()));
        page.on("pageerror", error => console.error(error));
        await page.goto(new URL(url, server.resolvedUrls.local[0]).href);
        await page.waitForFunction(() => window.checks !== undefined);
        await Promise.race([
            page.evaluate(() => window.checks),
            new Promise((_, reject) => setTimeout(() => reject(new Error("Browser tests timed out")),
                Number(process.env.WASM_BINDGEN_TEST_TIMEOUT ?? 120) * 1000).unref()),
        ]);
    } finally {
        if (page) {
            const output = await page.locator("#output").textContent({ timeout: 1000 }).catch(() => null);
            if (output) console.log(output);
        }
        try {
            await browser?.close();
        } finally {
            await server.close();
        }
    }
}

if (import.meta.main) await run();
