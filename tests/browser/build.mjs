import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";

const directory = fileURLToPath(new URL(".", import.meta.url));
const workspace = path.resolve(directory, "../..");
const target = path.resolve(workspace, process.env.CARGO_TARGET_DIR ?? "target/browser");
const env = {
    CARGO_TARGET_DIR: target,
    CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER: `node ${path.join(directory, "wasm-test.mjs")}`,
    WASM_BINDGEN_TEST_TIMEOUT: process.env.WASM_BINDGEN_TEST_TIMEOUT ?? "120",
    CARGO_ENCODED_RUSTFLAGS: [
        "-C", "target-feature=+atomics,+bulk-memory,+mutable-globals",
        "-C", "link-arg=--shared-memory",
        "-C", "link-arg=--import-memory",
        // Shared memories require a maximum; allow the full Wasm32 address space.
        "-C", "link-arg=--max-memory=4294967296",
        // wasm-bindgen uses these linker symbols to initialize each thread's TLS.
        "-C", "link-arg=--export=__wasm_init_tls",
        "-C", "link-arg=--export=__tls_base",
        "-C", "link-arg=--export=__tls_size",
        "-C", "link-arg=--export=__tls_align",
    ].join("\u001f"),
};

function run(command, args) {
    const result = spawnSync(command, args, {
        cwd: workspace, env: { ...process.env, ...env }, stdio: "inherit",
    });
    if (result.error) throw result.error;
    if (result.status !== 0) process.exit(result.status ?? 1);
}

export function cargo(command, args) {
    run("cargo", [
        "+nightly-2026-10-20", command, "--locked",
        "--target", "wasm32-unknown-unknown", "-Z", "build-std=std,panic_abort",
        ...args,
    ]);
}

export function build() {
    cargo("build", ["-p", "relocade-browser-tests"]);
    run("wasm-bindgen", [
        path.join(target, "wasm32-unknown-unknown/debug/relocade_browser_tests.wasm"),
        "--target", "web", "--out-dir", path.join(directory, "pkg"), "--no-typescript",
    ]);
}

if (import.meta.main) build();
