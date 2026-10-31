import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";

const workspace = fileURLToPath(new URL("..", import.meta.url));
export const target = path.resolve(workspace, process.env.CARGO_TARGET_DIR ?? "target/browser");
const env = {
    CARGO_TARGET_DIR: target,
    WASM_BINDGEN_TEST_TIMEOUT: process.env.WASM_BINDGEN_TEST_TIMEOUT ?? "120",
    CARGO_ENCODED_RUSTFLAGS: [
        "-C", "target-feature=+atomics,+bulk-memory,+mutable-globals",
        "-C", "link-arg=--growable-table",
        "-C", "link-arg=--export=__stack_pointer",
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

function run(command, args, extraEnv = {}) {
    const result = spawnSync(command, args, {
        cwd: workspace, env: { ...process.env, ...env, ...extraEnv }, stdio: "inherit",
    });
    if (result.error) throw result.error;
    if (result.status !== 0) throw new Error(`${command} exited with ${result.status}`);
}

export function cargo(command, args, runner) {
    run("cargo", [
        "+nightly-2026-10-20", command, "--locked",
        "--target", "wasm32-unknown-unknown", "-Z", "build-std=std,panic_abort",
        ...args,
    ], runner ? { CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER: runner } : {});
}

export function bindgen(wasm, output) {
    run("wasm-bindgen", [wasm, "--target", "web", "--out-dir", output,
        "--no-typescript"]);
}
