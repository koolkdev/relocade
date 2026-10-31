import assert from "node:assert/strict";
import { readFile, readdir } from "node:fs/promises";
import path from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { gunzipSync } from "node:zlib";
import { build as bundle, preview } from "vite";
import { chromium } from "playwright";
import { build } from "../../tools/web.mjs";

// Literal firmware glyphs: verify the displayed pixels, without a test-only
// text/status API or a software rasterizer interpreting the same capture.
const promptRow = 6;
const prompt = [96, 48, 24, 12, 24, 48, 96, 0];
const xGlyph = [0, 0, 198, 108, 56, 108, 198, 0];

const config = build("relocade-dos-app");
await bundle(config);
const headers = await readFile(path.join(config.build.outDir, "_headers"), "utf8");
assert.match(headers, /Cross-Origin-Opener-Policy: same-origin/);
assert.match(headers, /Cross-Origin-Embedder-Policy: require-corp/);
const files = await readdir(config.build.outDir, { recursive: true });
const wasms = files.filter(file => file.endsWith(".wasm"));
assert.ok(wasms.length > 0);
for (const file of wasms) {
    const wasm = gunzipSync(await readFile(path.join(config.build.outDir, file)));
    assert.deepEqual([...wasm.subarray(0, 8)], [0, 97, 115, 109, 1, 0, 0, 0]);
    assert.ok(headers.includes(`/${file}\n  Content-Encoding: gzip\n  Content-Type: application/wasm\n`));
}
const server = await preview({ ...config, preview: { host: "127.0.0.1", port: 0 } });
let browser;
try {
    browser = await chromium.launch({
        executablePath: process.env.CHROME_BINARY,
        args: ["--enable-unsafe-webgpu", "--use-angle=swiftshader", "--enable-unsafe-swiftshader",
            "--use-webgpu-adapter=opengles"],
    });
    const page = await browser.newPage({ viewport: { width: 1040, height: 900 } });
    // The fixture serves no favicon for the browser's automatic request.
    await page.route("**/favicon.ico", route => route.fulfill({ status: 204 }));
    const errors = [];
    page.on("pageerror", error => errors.push(error.message));
    page.on("console", message => {
        if (message.type() === "error") errors.push(message.text());
    });
    await page.goto(server.resolvedUrls.local[0]);
    await displayedGlyph(page, 0, promptRow, prompt);
    await page.locator("#screen").click();
    await page.keyboard.type("echo xy");
    await page.keyboard.press("Backspace");
    await page.keyboard.press("Enter");
    await displayedGlyph(page, 0, promptRow + 1, xGlyph);
    await displayedGlyph(page, 0, promptRow + 2, prompt);
    await page.setViewportSize({ width: 960, height: 768 });
    await displayedGlyph(page, 0, promptRow + 1, xGlyph);
    await page.keyboard.type("echo x");
    await page.keyboard.press("Enter");
    await displayedGlyph(page, 0, promptRow + 3, xGlyph);
    await displayedGlyph(page, 0, promptRow + 4, prompt);
    assert.deepEqual(errors, []);
    console.log("DOS console: keyboard editing, displayed echo and resize passed");
} finally {
    await browser?.close();
    await server.close();
}

async function displayedGlyph(page, column, row, expected) {
    const deadline = Date.now() + 30_000;
    let actual;
    do {
        const png = await page.locator("#screen").screenshot();
        actual = await page.evaluate(async ({ base64, column, row }) => {
            const blob = await (await fetch(`data:image/png;base64,${base64}`)).blob();
            const image = await createImageBitmap(blob);
            const canvas = document.createElement("canvas");
            canvas.width = image.width;
            canvas.height = image.height;
            const context = canvas.getContext("2d");
            context.drawImage(image, 0, 0);
            image.close();
            const { data } = context.getImageData(0, 0, canvas.width, canvas.height);
            const width = Math.min(canvas.width, canvas.height * 4 / 3);
            const height = width * 3 / 4;
            const left = (canvas.width - width) / 2;
            const top = (canvas.height - height) / 2;
            return Array.from({ length: 8 }, (_, scan) => {
                let bits = 0;
                for (let bit = 0; bit < 8; bit++) {
                    const x = Math.floor(left + (column * 8 + bit + 0.5) * width / 640);
                    const y = Math.floor(top + (row * 8 + scan + 0.5) * height / 200);
                    if (data[(y * canvas.width + x) * 4] > 100) bits |= 0x80 >> bit;
                }
                return bits;
            });
        }, { base64: png.toString("base64"), column, row });
        if (actual.join() === expected.join()) return;
        await delay(50);
    } while (Date.now() < deadline);
    assert.deepEqual(actual, expected, `displayed glyph at (${column}, ${row})`);
}
