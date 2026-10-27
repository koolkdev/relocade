import assert from "node:assert/strict";
import { setTimeout as delay } from "node:timers/promises";
import { withBrowser } from "../../../../tools/web/browser.mjs";
import config from "./vite.config.mjs";

const timeout = 120_000;

function program(running) {
    const bytes = [
        0xb8, 0, 0xb8, 0x8e, 0xc0, // ES = B800.
        0x31, 0xff, 0xfc, // DI = 0; CLD.
        0xb8, 0x20, 0x10, // Blue background with spaces.
        0xb9, 0xc0, 3, 0xf3, 0xab, // Fill 12 rows.
        0xb8, 0x20, 0x20, // Green background with spaces.
        0xb9, 0x10, 4, 0xf3, 0xab, // Fill the remaining 13 rows.
    ];
    if (running) bytes.push(0xeb, 0xfe); // Keep executing until stopped.
    bytes.push(0xb8, 7, 0x4c, 0xcd, 0x21); // Exit 7.
    return bytes;
}

async function waitUntil(description, check) {
    const deadline = Date.now() + timeout;
    while (!await check()) {
        assert.ok(Date.now() < deadline, `Timed out waiting for ${description}`);
        await delay(50);
    }
}

async function assertActive(page) {
    const outcome = await page.evaluate(() => window.outcome);
    assert.equal(outcome, undefined, `Application closed before presentation: ${JSON.stringify(outcome)}`);
}

async function waitForFrame(page, width, height, points) {
    let observed;
    await waitUntil(`a ${width}×${height} VGA frame`, async () => {
        await assertActive(page);
        // Inspect the composited display, including the OffscreenCanvas handoff.
        const png = await page.locator("#relocade-screen").screenshot().catch(async error => {
            await assertActive(page);
            throw error;
        });
        observed = await page.evaluate(async ({ data, points }) => {
            // Check the bitmap too: CSS alone could scale a stale bitmap.
            const bitmap = await createImageBitmap(document.querySelector("#relocade-screen"));
            const bitmapSize = [bitmap.width, bitmap.height];
            bitmap.close();
            const image = new Image();
            image.src = `data:image/png;base64,${data}`;
            await image.decode();
            const canvas = document.createElement("canvas");
            canvas.width = image.naturalWidth;
            canvas.height = image.naturalHeight;
            const context = canvas.getContext("2d");
            context.drawImage(image, 0, 0);
            return {
                width: canvas.width,
                height: canvas.height,
                bitmapSize,
                pixels: points.map(([x, y]) => [...context.getImageData(x, y, 1, 1).data].slice(0, 3)),
            };
        }, { data: png.toString("base64"), points });
        return observed.width === width && observed.height === height
            && observed.bitmapSize[0] === width && observed.bitmapSize[1] === height
            && points.every(([, , color], i) => color.every((value, c) => observed.pixels[i][c] === value));
    }).catch(error => {
        throw new Error(`${error.message}; last frame: ${JSON.stringify(observed)}`, { cause: error });
    });
}

await withBrowser(config, async (page, baseUrl) => {
    const errors = [];
    const workers = new Set();
    let createdWorkers = 0;
    page.on("pageerror", error => errors.push(error.message));
    page.on("worker", worker => {
        createdWorkers++;
        workers.add(worker);
        worker.on("close", () => workers.delete(worker));
    });
    page.setDefaultTimeout(timeout);
    await page.setViewportSize({ width: 1280, height: 1100 });
    await page.goto(baseUrl);
    assert.equal(await page.evaluate(() => crossOriginIsolated), true);
    await page.waitForFunction(() => typeof window.openApplication === "function");

    // Exercise the supplied-byte entrypoint directly, with no loading or input UI.
    for (const running of [false, true]) {
        const before = createdWorkers;
        await page.evaluate(bytes => {
            window.outcome = undefined;
            window.application = window.openApplication(new Uint8Array(bytes));
            window.application.finished.then(
                code => window.outcome = { code },
                error => window.outcome = { error: error instanceof Error ? error.message : String(error) },
            );
        }, program(running));
        await page.locator("#relocade-screen").waitFor();
        assert.equal(await page.locator("#relocade-screen").count(), 1);
        await waitForFrame(page, 960, 720, [
            [100, 100, [0, 0, 170]], [100, 320, [0, 0, 170]], [100, 600, [0, 170, 0]],
        ]);
        assert.equal(createdWorkers - before, 2, "one execution worker and one renderer worker");
        await page.locator("#relocade-screen").evaluate(canvas => {
            canvas.style.width = "800px";
            canvas.style.height = "600px";
        });
        await waitForFrame(page, 800, 600, [
            [100, 100, [0, 0, 170]], [100, 320, [0, 170, 0]], [100, 500, [0, 170, 0]],
        ]);
        await page.evaluate(() => window.application.stop());
        await page.waitForFunction(() => window.outcome !== undefined);
        const outcome = await page.evaluate(() => window.outcome);
        assert.deepEqual(outcome, { code: running ? 0 : 7 });
        await page.evaluate(() => window.application.free());
        await waitUntil("both workers to close", () => workers.size === 0);
        assert.equal(await page.locator("#relocade-screen").count(), 0);
    }
    assert.deepEqual(errors, [], "no uncaught browser errors");
    console.log("DOS visualization, resize and worker cleanup passed.");
}, { built: true });
