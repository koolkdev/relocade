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

    await page.goto(`${address}?dos`);
    await page.waitForFunction(() => window.start_stress !== undefined);
    await page.evaluate(() => {
        window.control = window.start_stress();
        window.result = window.control.finished.then(() => ({ ok: true }), error => ({ error: String(error) }));
    });
    await canvas.waitFor({ state: "visible" });
    await page.waitForFunction(() => window.stress_state().sequence > 2);
    await canvas.click();
    // Hidden output stops drawing while the guest and VGA continue advancing.
    await canvas.evaluate(canvas => { canvas.style.display = 'none'; });
    await page.waitForTimeout(250);
    const hidden = await page.evaluate(() => window.stress_state());
    await page.waitForFunction(before => {
        const now = window.stress_state();
        return now.sequence > before.sequence + 4 && now.counter !== before.counter;
    }, hidden);
    assert.equal((await page.evaluate(() => window.stress_state())).draws, hidden.draws);
    await canvas.evaluate(canvas => { canvas.style.display = 'block'; });
    await page.waitForFunction(draws => window.stress_state().draws > draws, hidden.draws);
    await canvas.click();
    await page.evaluate(() => window.hold_graphics(true));
    await page.waitForFunction(() => window.stress_state().held === 1);
    try {
        const before = await page.evaluate(() => window.stress_state());
        await page.waitForFunction(before => {
            const now = window.stress_state();
            return now.sequence > before.sequence + 3 && now.counter !== before.counter;
        }, before);
        assert.equal((await page.evaluate(() => window.stress_state())).draws, before.draws, "drawing escaped its gate");
        // Give winit each resize while the graphics owner remains occupied.
        for (let i = 0; i < 20; i++) {
            await canvas.evaluate((canvas, width) => {
                canvas.style.width = `${width}px`;
                canvas.style.height = '600px';
                return new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
            }, 700 + i * 5);
        }
        await canvas.evaluate(canvas => {
            canvas.style.width = '800px';
            return new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
        });
        await page.keyboard.press('y');
        await page.waitForFunction(() => window.stress_state().color === 0x22);
    } finally {
        await page.evaluate(() => window.hold_graphics(false));
    }
    await page.waitForFunction(() => window.stress_state().nextWidth !== 0);
    const resized = await page.evaluate(() => window.stress_state());
    assert.deepEqual([resized.nextWidth, resized.nextHeight], [800, 600], "next demand replayed stale dimensions");
    await waitForCell([0, 170, 0]);

    // A real device-loss callback must close both workers, including idle DOS.
    await page.evaluate(() => window.destroy_device());
    const lost = await page.evaluate(() => window.result);
    assert.match(lost.error, /GPU|Surface/);
    assert.equal((await page.evaluate(() => window.stress_state())).dropped, 3);
    assert.equal(await canvas.count(), 0);
    assert.deepEqual(errors, []);
    console.log("Slow graphics, bounded resize, hidden timing and device-loss cleanup passed.");

    // CDP changes DPR but Chromium's physical-box observer ignores that emulation.
    // Exercise winit's supported CSS-size/DPR fallback (used by Safari) explicitly.
    await page.addInitScript(() => { delete ResizeObserverEntry.prototype.devicePixelContentBoxSize; });
    await page.goto(address);
    await page.waitForFunction(() => window.image_draws?.() > 2);
    const logicalSize = await canvas.evaluate(canvas => [canvas.clientWidth, canvas.clientHeight]);
    const cdp = await page.context().newCDPSession(page);
    await cdp.send('Emulation.setDeviceMetricsOverride', { width: 800, height: 600, deviceScaleFactor: 2, mobile: false });
    await page.waitForFunction(([width, height]) => {
        const canvas = document.querySelector('canvas');
        return canvas.width === width * 2 && canvas.height === height * 2;
    }, logicalSize);
    await cdp.send('Emulation.clearDeviceMetricsOverride');
    await cdp.detach();
    await page.evaluate(() => window.control.stop());
    await page.evaluate(() => window.finished);
    assert.equal(await canvas.count(), 0);
    assert.deepEqual(errors, []);
    console.log("Browser scale-factor fallback and shutdown passed.");

    await page.goto(`${address}?cancel`);
    await page.waitForFunction(() => window.finished !== undefined);
    await page.evaluate(() => window.finished);
    assert.equal(await canvas.count(), 0);
    const cancelled = Date.now() + 10_000;
    while (page.workers().length > 0) {
        assert(Date.now() < cancelled, "startup cancellation left workers running");
        await page.waitForTimeout(20);
    }
    assert.deepEqual(errors, []);
    console.log("Cancellation during startup passed.");
});
