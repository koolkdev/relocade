import * as bindings from "relocade:wasm-bindings";

export async function prepareTests() {
    globalThis.__wbg_test_invoke = f => f();
    const wasm = await bindings.default();
    return async ({ names, ignored }) => {
        if (!crossOriginIsolated) throw new Error("Browser tests require cross-origin isolation");
        const selected = new Set(names);
        const allTests = Object.keys(wasm).filter(name => name.startsWith("__wbgt_"));
        const tests = allTests.filter(name => selected.has(name.slice(name.indexOf("::") + 2)));
        if (tests.length !== selected.size) throw new Error("Missing test exports");

        // wasm-bindgen-test owns assertion capture, ignored tests and reporting.
        for (const method of ["debug", "log", "info", "warn", "error"]) {
            const original = console[method];
            console[method] = (...args) => {
                bindings[`__wbgtest_console_${method}`](args);
                original.apply(console, args);
            };
        }
        const context = new bindings.WasmBindgenTestContext(false);
        context.include_ignored(ignored);
        context.filtered_count(allTests.length - tests.length);
        try {
            if (!await context.run(tests.map(name => wasm[name]))) throw new Error("Browser tests failed");
        } finally {
            context.free();
        }
    };
}
