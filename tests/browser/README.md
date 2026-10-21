# Browser tests

All browser tests use the same threaded Rust build: shared host memory, atomics,
and per-thread TLS. `npm test` runs the existing runtime-web and renderer tests
through wasm-bindgen-test, then the bundled worker integration checks through
Vite and Chrome. Both stages share the toolchain, flags and Cargo target directory.
The bundled checks exercise the worker bootstrap used by browser applications.

The worker checks cover Rust channel wakeups after a task returns Pending,
independent TLS and worker-local Rc state, shared-memory messages that preserve
later growth and writes, and compiled-module handoff between execution owners.
Compiler lowering and compilation errors are checked natively.

From this directory:

```sh
rustup toolchain install nightly-2026-10-20 --profile minimal --component rust-src
cargo install wasm-bindgen-cli --version 0.2.129 --locked
npm ci
CHROME_BINARY=/path/to/chrome CHROMEDRIVER=/path/to/chromedriver npm test
```

Use Chrome and a matching ChromeDriver. The entrypoint selects `CHROME_BINARY`
for both runners and configures software WebGPU for the renderer tests. Set
`WASM_BINDGEN_TEST_WEBDRIVER_JSON` to supply your own WebDriver configuration.
CI uses the same entrypoint in [the browser job](../../.github/workflows/tests.yml).

Cargo test filters can be passed to `npm test`, for example
`npm test -- --test compilation`. The worker checks still run afterward.
For only the worker checks, run `npm run build && npm run test:workers`.
`npm run dev` opens those checks for manual inspection after building them.
The build script defaults to `target/browser` and honors `CARGO_TARGET_DIR`.
Vite supplies the required isolation headers and worker binding alias.

The same worker checks also run on native threads:

```sh
cargo test -p relocade-browser-tests --locked
```

See the [worker guide](../../crates/runtime/docs/workers.md) for message and
resource ownership. Add future cross-thread integration checks here so CPU,
renderer and device workers share this setup.
