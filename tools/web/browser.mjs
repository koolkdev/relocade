import { createServer } from "vite";
import { chromium } from "playwright";

export async function withBrowser(options, check) {
    const server = await createServer({ ...options, server: { ...options.server, host: "127.0.0.1", port: 0 } });
    await server.listen();
    let browser;
    try {
        browser = await chromium.launch({
            executablePath: process.env.CHROME_BINARY,
            // Keep WebGPU and canvas composition on the same software GL backend.
            args: ["--enable-unsafe-webgpu", "--use-angle=swiftshader", "--enable-unsafe-swiftshader",
                "--use-webgpu-adapter=opengles", "--js-flags=--expose-gc"],
        });
        const page = await browser.newPage();
        page.on("console", message => console.log(message.text()));
        page.on("pageerror", error => console.error(error));
        await check(page, server.resolvedUrls.local[0]);
    } finally {
        try { await browser?.close(); } finally { await server.close(); }
    }
}
