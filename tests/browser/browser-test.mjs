import { createServer, normalizePath } from "vite";
import { chromium } from "playwright";
import { createServer as createHttpServer } from "node:http";
import { fileURLToPath } from "node:url";

const defaultBindings = fileURLToPath(new URL("./pkg/relocade_browser_tests.js", import.meta.url));
const bindingsId = "\0relocade-test-bindings";

// Cargo starts a runner process for each binary. Keep the server and browser in
// their parent process; each binary still gets its own browser context and workers.
export async function withBrowser(consume) {
    let bindings = defaultBindings;
    const server = await createServer({
        configFile: fileURLToPath(new URL("./vite.config.mjs", import.meta.url)),
        resolve: { alias: { "relocade:wasm-bindings": bindingsId } },
        plugins: [{
            name: "relocade-test-bindings",
            resolveId(id) {
                if (id === bindingsId) return id;
            },
            load(id) {
                if (id === bindingsId) {
                    const source = JSON.stringify(normalizePath(bindings));
                    return `export * from ${source}; export { default } from ${source};`;
                }
            },
        }],
        // Generated bindings and worker shims are local modules, with no npm imports.
        optimizeDeps: { noDiscovery: true, include: [] },
        server: { host: "127.0.0.1", port: 0, hmr: false },
    });
    let browser;
    let control;
    let pending = Promise.resolve();

    async function execute(url = "/", module = defaultBindings, activation) {
        bindings = module;
        server.environments.client.moduleGraph.invalidateAll();
        const context = await browser.newContext();
        let page;
        let timeout;
        try {
            page = await context.newPage();
            page.on("console", message => console.log(message.text()));
            page.on("pageerror", error => console.error(error));
            await page.goto(new URL(url, server.resolvedUrls.local[0]).href);
            if (activation) {
                await page.waitForFunction(() => window.ready === true);
                await page.click(activation);
            }
            await page.waitForFunction(() => window.checks !== undefined);
            await Promise.race([
                page.evaluate(() => window.checks),
                new Promise((_, reject) => {
                    timeout = setTimeout(() => reject(new Error("Browser tests timed out")),
                        Number(process.env.WASM_BINDGEN_TEST_TIMEOUT ?? 120) * 1000).unref();
                }),
            ]);
        } finally {
            clearTimeout(timeout);
            if (page) {
                const output = await page.locator("#output").textContent({ timeout: 1000 }).catch(() => null);
                if (output) console.log(output);
            }
            await context.close();
        }
    }

    function run(...args) {
        const result = pending.then(() => execute(...args));
        pending = result.catch(() => {});
        return result;
    }

    try {
        await server.listen();
        browser = await chromium.launch({
            executablePath: process.env.CHROME_BINARY,
            // Match WebGPU and canvas composition with the same software GL backend.
            args: ["--enable-unsafe-webgpu", "--use-angle=swiftshader", "--enable-unsafe-swiftshader",
                "--use-webgpu-adapter=opengles", "--js-flags=--expose-gc"],
        });
        control = createHttpServer(async (request, response) => {
            response.setHeader("Connection", "close");
            if (request.method !== "POST" || request.url !== "/") {
                response.writeHead(405).end();
                return;
            }
            try {
                let body = "";
                for await (const chunk of request) body += chunk;
                const { url, bindings, activation } = JSON.parse(body);
                await run(url, bindings, activation);
                response.writeHead(200).end();
            } catch (error) {
                response.writeHead(500).end(error.stack ?? String(error));
            }
        });
        await new Promise((resolve, reject) => {
            control.once("error", reject);
            control.listen(0, "127.0.0.1", resolve);
        });
        return await consume({ run, endpoint: `http://127.0.0.1:${control.address().port}/` });
    } finally {
        if (control) await new Promise(resolve => control.close(resolve));
        try {
            await browser?.close();
        } finally {
            await server.close();
        }
    }
}

export function run(...args) {
    return withBrowser(browser => browser.run(...args));
}

if (import.meta.main) await run();
