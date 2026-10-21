// Cargo runner: keep wasm-bindgen-test's filtering and runtime, but serve its
// bindings through Vite so tests can start the same workers as the application.
import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { run } from "./browser-test.mjs";

const [wasm, ...args] = process.argv.slice(2);
if (args.includes("--bench")) throw new Error("This runner supports tests only; benchmarks require wasm-bindgen-test-runner");
const listing = spawnSync("wasm-bindgen-test-runner", [wasm, ...args.filter(arg => arg !== "--list"), "--list"], {
    encoding: "utf8", stdio: ["inherit", "pipe", "inherit"],
});
if (listing.error) throw listing.error;
if (listing.status !== 0) process.exit(listing.status ?? 1);
if (args.includes("--list")) {
    process.stdout.write(listing.stdout);
    process.exit(0);
}
const tests = listing.stdout.split("\n")
    .filter(line => line.endsWith(": test"))
    .map(line => line.slice(0, -6));
if (!tests.length) {
    console.log("no tests to run!");
    process.exit(0);
}

const root = fileURLToPath(new URL(".", import.meta.url));
mkdirSync(path.join(root, "pkg"), { recursive: true });
const directory = mkdtempSync(path.join(root, "pkg/test-"));
try {
    const bindings = spawnSync("wasm-bindgen", [
        wasm, "--target", "web", "--out-dir", directory,
        "--out-name", "suite", "--no-typescript",
    ], { stdio: "inherit" });
    if (bindings.error) throw bindings.error;
    if (bindings.status !== 0) throw new Error(`wasm-bindgen exited with ${bindings.status}`);
    const parameters = new URLSearchParams({
        tests: JSON.stringify(tests),
        ignored: String(args.includes("--ignored") || args.includes("--include-ignored")),
    });
    await run(`/wasm-test.html?${parameters}`, path.join(directory, "suite.js"));
} finally {
    rmSync(directory, { recursive: true, force: true });
}
