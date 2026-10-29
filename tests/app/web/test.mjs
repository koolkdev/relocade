import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";
import { PNG } from "pngjs";
import { withBrowser } from "../../../tools/web/browser.mjs";

const same = (actual, expected) => actual.every((value, i) => Math.abs(value - expected[i]) <= 1);
function pixel(image, x, y) {
    return [...image.data.subarray((y * image.width + x) * 4, (y * image.width + x) * 4 + 3)];
}
await withBrowser({ configFile: fileURLToPath(new URL("./vite.config.mjs", import.meta.url)) }, async (page, address) => {
    const errors = [];
    page.on("pageerror", error => errors.push(error.message));
    await page.goto(address);
    await page.waitForFunction(() => window.control !== undefined);
    const canvas = page.locator("canvas");
    await canvas.waitFor({ state: "visible" });
    assert.deepEqual(await canvas.evaluate(canvas => [canvas.clientWidth, canvas.clientHeight]), [640, 480], "harness overrode embedding CSS");
    const deadline = Date.now() + 30_000;
    while (true) {
        const image = PNG.sync.read(await canvas.screenshot());
        const center = image.width / 2;
        const size = Math.min(image.width, image.height);
        if (same(pixel(image, Math.floor(center - size / 4), Math.floor(image.height / 4)), [170, 0, 0]) &&
            same(pixel(image, Math.floor(center + size / 4), Math.floor(3 * image.height / 4)), [255, 255, 85])) break;
        assert(Date.now() < deadline, "test image never reached the displayed canvas");
        await page.waitForTimeout(50);
    }
    // Sustained host redraws must leave the graphics worker's JS task queue alive.
    await page.waitForFunction(() => window.image_draws() >= 120 && window.image_worker_tasks() >= 10);
    await page.evaluate(() => window.control.stop());
    assert.equal(await page.evaluate(() => window.finished), 7);
    assert.equal(await page.evaluate(() => window.image_dropped()), 3);
    assert.equal(await canvas.count(), 0);
    await page.waitForFunction(() => document.querySelector("canvas") === null);
    const stopped = Date.now() + 10_000;
    while (page.workers().length > 0) {
        assert(Date.now() < stopped, "browser workers were not terminated");
        await page.waitForTimeout(20);
    }
    assert.deepEqual(errors, []);
    console.log("Displayed test image and orderly worker/canvas cleanup passed.");

    await page.goto(`${address}?dos`);
    await page.waitForFunction(() => window.start_dos !== undefined);
    await page.evaluate(() => {
        // Red cell, wait for 'y' through DOS, green cell, then program exit.
        const program = new Uint8Array([
            0xb8, 0x00, 0xb8, 0x8e, 0xc0,
            0x26, 0xc7, 0x06, 0, 0, 0xdb, 0x44,
            0xb4, 0x07, 0xcd, 0x21, 0x3c, 0x79, 0x75, 0xf8,
            0x26, 0xc7, 0x06, 0, 0, 0xdb, 0x22,
            0xb8, 0, 0x4c, 0xcd, 0x21,
        ]);
        window.control = window.start_dos(program);
        window.finished = window.control.finished;
    });
    await canvas.waitFor({ state: "visible" });
    async function waitForCell(color) {
        const deadline = Date.now() + 30_000;
        while (!same(pixel(PNG.sync.read(await canvas.screenshot()), 4, 4), color)) {
            assert(Date.now() < deadline, `DOS cell never became ${color}`);
            await page.waitForTimeout(50);
        }
    }
    await waitForCell([170, 0, 0]);
    await canvas.click();
    await page.keyboard.press("y");
    await waitForCell([0, 170, 0]);
    // Exit retains its final image; a later host redraw still uses it.
    await page.waitForTimeout(250);
    await waitForCell([0, 170, 0]);
    await page.evaluate(() => window.control.stop());
    await page.evaluate(() => window.finished);
    assert.equal(await canvas.count(), 0);
    assert.deepEqual(errors, []);
    console.log("Displayed DOS input and retained output after program exit passed.");
});
