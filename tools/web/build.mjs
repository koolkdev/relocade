import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";

const directory = fileURLToPath(new URL(".", import.meta.url));
const workspace = path.resolve(directory, "../..");
const target = path.resolve(workspace, process.env.CARGO_TARGET_DIR ?? "target/browser");
const env = {
    CARGO_TARGET_DIR: target,
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

function run(command, args, environment = {}) {
    const result = spawnSync(command, args, {
        cwd: workspace, env: { ...process.env, ...env, ...environment }, stdio: "inherit",
    });
    if (result.error) throw result.error;
    if (result.status !== 0) process.exit(result.status ?? 1);
}

export function cargo(command, args, environment) {
    run("cargo", [
        "+nightly-2026-10-20", command, "--locked",
        "--target", "wasm32-unknown-unknown", "-Z", "build-std=std,panic_abort",
        ...args,
    ], environment);
}

export function buildBindings(packageName, output) {
    // Match runtime tests while retaining debug assertions and symbols.
    cargo("build", ["-p", packageName, "--lib", "--profile", "test"]);
    run("wasm-bindgen", [
        path.join(target, `wasm32-unknown-unknown/debug/${packageName.replaceAll("-", "_")}.wasm`),
        "--target", "web", "--out-dir", output, "--no-typescript",
    ]);
}
