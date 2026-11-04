import assert from "node:assert/strict";
import { test } from "node:test";
import { mkdir, mkdtemp, writeFile, rm } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { withBrowser } from "./browser-test.mjs";

test("shared browser switches page and worker bindings and isolates each suite", async () => {
    const root = fileURLToPath(new URL("./pkg/", import.meta.url));
    await mkdir(root, { recursive: true });
    const directory = await mkdtemp(path.join(root, "runner-"));
    let endpoint;
    try {
        await writeFile(path.join(directory, "worker.mjs"), `
            import { identity } from "web-thread:wasm-shim";
            postMessage(identity);
        `);
        await writeFile(path.join(directory, "check.html"), `
            <link rel="icon" href="data:,">
            <pre id="output"></pre>
            <script type="module">
                import { identity } from "relocade:wasm-bindings";
                window.checks = (async () => {
                    if (!crossOriginIsolated) throw new Error("Missing cross-origin isolation");
                    const parameters = new URLSearchParams(location.search);
                    const expected = parameters.get("expected");
                    if (identity !== expected) throw new Error("Stale page bindings");
                    if (localStorage.getItem("previous") || window.previous) {
                        throw new Error("Suite state leaked into another browser context");
                    }
                    localStorage.setItem("previous", expected);
                    window.previous = expected;
                    const worker = new Worker(new URL("./worker.mjs", import.meta.url), { type: "module" });
                    try {
                        const actual = await new Promise((resolve, reject) => {
                            worker.onmessage = event => resolve(event.data);
                            worker.onerror = reject;
                        });
                        if (actual !== expected) throw new Error("Stale worker bindings");
                        if (performance.getEntriesByType("resource").some(resource =>
                            new URL(resource.name).pathname === "/@vite/client")) {
                            throw new Error("Test page loaded the hot-reload client");
                        }
                    } finally {
                        worker.terminate();
                    }
                    if (parameters.has("fail")) throw new Error("Intentional suite failure");
                })();
            </script>
        `);
        for (const name of ["first", "second"]) {
            await writeFile(path.join(directory, `${name}.mjs`), `
                export const identity = ${JSON.stringify(name)};
                export default async function initialize() {}
            `);
        }
        const request = name => ({
            url: `/pkg/${path.basename(directory)}/check.html?expected=${name}`,
            bindings: path.join(directory, `${name}.mjs`),
        });
        await withBrowser(async browser => {
            endpoint = browser.endpoint;
            const invoke = body => fetch(endpoint, {
                method: "POST", body: JSON.stringify(body),
            });
            assert.equal((await invoke(request("first"))).status, 200);
            const second = request("second");
            const failed = await invoke({ ...second, url: `${second.url}&fail` });
            assert.equal(failed.status, 500);
            assert.match(await failed.text(), /Intentional suite failure/);
            assert.equal((await invoke(second)).status, 200);
            const first = request("first");
            await browser.run(first.url, first.bindings);
        });
        await assert.rejects(fetch(endpoint));
    } finally {
        await rm(directory, { recursive: true, force: true });
    }
});
