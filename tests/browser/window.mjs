import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";
import { buildBindings } from "./build.mjs";
import { withBrowser } from "./browser-test.mjs";

buildBindings("relocade-window-example", fileURLToPath(new URL("./pkg-window", import.meta.url)));
if (!process.argv.includes("--build-only")) await withBrowser(async (page, address) => {
    const errors = [];
    let workers = 0;
    page.on("pageerror", error => errors.push(error.message));
    page.on("worker", () => workers++);
    await page.goto(new URL("/window.html", address).href);
    const canvas = page.locator("canvas");
    await canvas.waitFor({ state: "visible" });
    assert.deepEqual(await canvas.evaluate(canvas => [canvas.clientWidth, canvas.clientHeight]), [640, 480]);
    await page.waitForFunction(() => document.querySelector("#output").textContent.includes("Window 640x480"));
    await canvas.click();
    await page.keyboard.press("y");
    await page.waitForFunction(() => document.querySelector("#output").textContent.includes("key=y"));
    const before = await page.locator("#output").textContent();
    await canvas.evaluate(canvas => {
        canvas.style.width = '512px';
        canvas.style.height = '320px';
    });
    await page.waitForFunction(() => document.querySelector("#output").textContent.includes("Window 512x320"));
    const after = await page.locator("#output").textContent();
    assert(Number(after.match(/redraws=(\d+)/)[1]) > Number(before.match(/redraws=(\d+)/)[1]));
    assert.match(after, /key=y/);
    await page.keyboard.press("Escape");
    await canvas.waitFor({ state: "detached" });
    assert.equal(await page.locator("#output").textContent(), "Closed");
    assert.equal(workers, 0);
    assert.deepEqual(errors, []);
    console.log("Main-thread canvas input, resize, redraw and cleanup passed.");
});
