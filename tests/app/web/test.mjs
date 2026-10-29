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
});
