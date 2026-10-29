import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";
import { buildBindings } from "../../tools/web/build.mjs";
import { run } from "./browser-test.mjs";

const directory = fileURLToPath(new URL("../../apps/dos/web/", import.meta.url));
buildBindings("relocade-dos-app", `${directory}/pkg`);
await run("/?program=/program.com", `${directory}/pkg/relocade_dos_app.js`, {
  configFile: `${directory}/vite.config.mjs`,
  inspect: async (page, address) => {
    // Await a DOS key, write a white A on red, then return the received character.
    const program = Buffer.from([
        0xb4, 0x08, 0xcd, 0x21, 0x88, 0xc3,
        0xb8, 0x00, 0xb8, 0x8e, 0xc0,
        0x26, 0xc7, 0x06, 0x00, 0x00, 0x41, 0x4f,
        0x88, 0xd8, 0xb4, 0x4c, 0xcd, 0x21,
    ]);
    await page.route(url => url.pathname === "/program.com", route => route.fulfill({ body: program }));
    await page.goto(address);
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
    const workersReady = async () => {
        const deadline = Date.now() + 15000;
        while (page.workers().length !== 2) {
            assert(Date.now() < deadline, "execution and graphics workers did not start");
            await page.waitForTimeout(30);
        }
    };
    await workersReady();
    await canvas.focus();
    const held = page.waitForEvent("console", { predicate: message => message.text() === "graphics held" });
    // Instrument only the test browser. The worker that submits GPU work blocks
    // synchronously for two seconds; the execution worker must still receive input.
    await Promise.all(page.workers().map(worker => worker.evaluate(() => {
        const submit = GPUQueue.prototype.submit;
        let held = false;
        globalThis.submissions = 0;
        GPUQueue.prototype.submit = function (commands) {
            globalThis.submissions++;
            if (!held) {
                held = true;
                console.log("graphics held");
                const deadline = performance.now() + 2000;
                while (performance.now() < deadline) { /* hold this worker */ }
            }
            return submit.call(this, commands);
        };
    })));
    await held;
    await page.keyboard.press("Shift+A");
    await page.waitForFunction(() => document.querySelector("canvas")?.getAttribute("alt")?.includes("exited 65"), null, { timeout: 1000 });
    for (let width = 640; width < 700; width += 4) await page.setViewportSize({ width, height: 480 });
    await page.setViewportSize({ width: 640, height: 480 });
    await page.waitForFunction(() => document.querySelector("canvas").width === 640);
    const deadline = Date.now() + 15000;
    while (JSON.stringify(await pixel()) !== JSON.stringify([170, 0, 0, 255])) {
        if (Date.now() >= deadline) assert.deepEqual(await pixel(), [170, 0, 0, 255], "canvas did not present guest VRAM");
        await page.waitForTimeout(30);
    }
    await page.waitForTimeout(300); // Sustained redraws must leave browser tasks responsive.
    assert.deepEqual(await pixel(), [170, 0, 0, 255]);
    const submissions = async () => (await Promise.all(page.workers().map(worker => worker.evaluate(() => globalThis.submissions)))).reduce((a, b) => a + b, 0);
    await canvas.evaluate(canvas => { canvas.style.display = "none"; });
    await page.waitForTimeout(200); // Let any outstanding draw settle.
    const hidden = await submissions();
    await page.waitForTimeout(200);
    assert.equal(await submissions(), hidden, "hidden output must stop GPU drawing");
    await canvas.evaluate(canvas => { canvas.style.display = "block"; });
    const resumed = Date.now() + 15000;
    while (await submissions() === hidden) {
        assert(Date.now() < resumed, "visible output did not resume drawing");
        await page.waitForTimeout(30);
    }
    assert.equal(await page.evaluate(() => window.dos.error()), undefined);
    await page.evaluate(() => window.dos.close());
    await canvas.waitFor({ state: "detached" });
    assert.equal(await page.evaluate(() => window.dos.error()), undefined);
    // A GPU validation failure must also cancel DOS while it awaits input and
    // release both workers before removing the canvas.
    await page.reload();
    await canvas.waitFor();
    await workersReady();
    await Promise.all(page.workers().map(worker => worker.evaluate(() => {
        const create = GPUDevice.prototype.createCommandEncoder;
        GPUDevice.prototype.createCommandEncoder = function (descriptor) {
            GPUDevice.prototype.createCommandEncoder = create;
            const encoder = create.call(this, descriptor);
            const buffer = this.createBuffer({ size: 4, usage: GPUBufferUsage.COPY_SRC | GPUBufferUsage.COPY_DST });
            encoder.copyBufferToBuffer(buffer, 0, buffer, 0, 4); // Invalid overlapping copy.
            return encoder;
        };
    })));
    await canvas.waitFor({ state: "detached" });
    assert.match(await page.evaluate(() => window.dos.error()), /GPU:/);
  },
});
