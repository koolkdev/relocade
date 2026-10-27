import { createServer, preview } from "vite";
import { chromium } from "playwright";

export async function withBrowser(config, check, { built = false } = {}) {
    const options = { ...config, configFile: false };
    const address = { host: "127.0.0.1", port: 0 };
    const server = built
        ? await preview({ ...options, preview: { ...config.preview, ...address } })
        : await createServer({ ...options, server: { ...config.server, ...address } });
    let browser;
    try {
        if (!built) await server.listen();
        browser = await chromium.launch({
            executablePath: process.env.CHROME_BINARY,
            args: ["--enable-unsafe-webgpu", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--js-flags=--expose-gc"],
        });
        const page = await browser.newPage();
        page.on("console", message => console.log(message.text()));
        page.on("pageerror", error => console.error(error));
        await check(page, server.resolvedUrls.local[0]);
    } finally {
        try {
            await browser?.close();
        } finally {
            await server.close();
        }
    }
}
