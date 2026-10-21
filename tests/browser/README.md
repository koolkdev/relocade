# Browser tests

All browser tests use the same threaded Rust build: shared host memory, atomics,
and per-thread TLS. `npm test` runs the existing wasm-runtime, runtime and renderer tests
with wasm-bindgen-test, then the worker integration checks. Both stages run
through Vite and Chrome, sharing the toolchain, flags, Cargo target directory
and worker bootstrap used by browser applications.

The worker checks cover Rust channel wakeups after a task returns Pending,
independent TLS and worker-local Rc state, shared-memory messages that preserve
later growth and writes, and compiled-module handoff between execution owners.

From this directory:

```sh
rustup toolchain install nightly-2026-10-20 --profile minimal --component rust-src
cargo install wasm-bindgen-cli --version 0.2.129 --locked
npm ci
CHROME_BINARY=/path/to/chrome npm test
```

The shared runner selects `CHROME_BINARY` and configures software WebGPU for
the renderer tests. ChromeDriver is not required.
CI uses the same entrypoint in [the browser job](../../.github/workflows/tests.yml).

Cargo test filters can be passed to `npm test`, for example
`npm test -- --test memory`. The worker checks still run afterward.
For only the worker checks, run `npm run build && npm run test:workers`.
`npm run dev` opens those checks for manual inspection after building them.
The build script defaults to `target/browser` and honors `CARGO_TARGET_DIR`.
Vite supplies the required isolation headers and worker binding alias.

The Cargo runner in `wasm-test.mjs` asks `wasm-bindgen-test-runner` to select the
tests, generates their bindings, and serves them through Vite. The standard
`WasmBindgenTestContext` executes tests and handles assertions, ignored tests and
results. This lets ordinary backend tests create workers without a second set of
test definitions or a test-only compilation path.

The same worker checks also run on native threads:

```sh
cargo test -p relocade-browser-tests --locked
```

See the [worker guide](../../crates/runtime/docs/workers.md) for message and
resource ownership. Add future cross-thread integration checks here so CPU,
renderer and device workers share this setup.
