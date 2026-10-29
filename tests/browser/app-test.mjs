import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";
import { buildBindings } from "../../tools/web/build.mjs";
import { run } from "./browser-test.mjs";

const directory = fileURLToPath(new URL("../../apps/dos/web/", import.meta.url));
buildBindings("relocade-dos-app", `${directory}/pkg`);
await run("/?program=/program.com", `${directory}/pkg/relocade_dos_app.js`, {
  configFile: `${directory}/vite.config.mjs`,
  inspect: async (page, address) => {
    // White A on red, then DOS exit 7. This runs through the production page/API.
    const program = Buffer.from([
        0xb8, 0x00, 0xb8, 0x8e, 0xc0,
        0x26, 0xc7, 0x06, 0x00, 0x00, 0x41, 0x4f,
        0xb8, 0x07, 0x4c, 0xcd, 0x21,
    ]);
    await page.route(url => url.pathname === "/program.com", route => route.fulfill({ body: program }));
    await page.goto(address);
    await page.waitForFunction(() => document.querySelector("canvas")?.getAttribute("alt")?.includes("exited 7"));
    const canvas = page.locator("canvas");
    await canvas.waitFor();
    // Copy the real displayed canvas into a test-only 2D canvas for observation.
    const pixel = () => page.evaluate(() => {
        const output = document.createElement("canvas");
        output.width = output.height = 1;
        const context = output.getContext("2d");
        context.drawImage(document.querySelector("canvas"), 0, 0, 1, 1, 0, 0, 1, 1);
        return [...context.getImageData(0, 0, 1, 1).data];
    });
    // Start with a 4:3 viewport, so the first cell is not behind a letterbox.
    await page.setViewportSize({ width: 800, height: 600 });
    await page.waitForFunction(() => {
        const canvas = document.querySelector("canvas");
        return canvas.width === 800 && canvas.height === 600;
    });
    const deadline = Date.now() + 15000;
    while (JSON.stringify(await pixel()) !== JSON.stringify([170, 0, 0, 255])) {
        if (Date.now() >= deadline) assert.deepEqual(await pixel(), [170, 0, 0, 255], "canvas did not present guest VRAM");
        await page.waitForTimeout(30);
    }
    for (let width = 640; width < 700; width += 4) await page.setViewportSize({ width, height: 480 });
    await page.setViewportSize({ width: 640, height: 480 });
    await page.waitForFunction(() => document.querySelector("canvas").width === 640);
    await page.waitForTimeout(300); // Sustained redraws must leave browser tasks responsive.
    assert.deepEqual(await pixel(), [170, 0, 0, 255]);
    assert.equal(await page.evaluate(() => window.dos.error()), undefined);
    await page.evaluate(() => window.dos.close());
    await canvas.waitFor({ state: "detached" });
    assert.equal(await page.evaluate(() => window.dos.error()), undefined);
  },
});
