# Browser tests

All browser tests use the same threaded Rust build: shared host memory, atomics,
and per-thread TLS. The linker also exports Rust's stack pointer and permits its
function table to grow, so typed runtime calls can stay entirely inside Wasm.
Arguments and scalar results pass directly; aggregate results follow Rust's C
return convention through generated Wasm glue. `npm test` runs the wasm-bindings,
wasm-runtime, machine, VGA and renderer tests with wasm-bindgen-test, then the worker
integration checks. Both stages run
through Vite and Chrome, sharing the [web tooling](../../tools/web/README.md)
and worker bootstrap used by browser applications.

The worker checks cover Rust channel wakeups after a task returns Pending,
independent TLS and worker-local Rc state, shared-memory messages that preserve
later growth and writes after the allocating worker exits, and compiled-module
handoff between execution owners. The [CPU worker example](src/cpus.rs) retains
caller access to mapped memory while a worker awaits Rust input, resumes guest
execution, and imports the retained allocation into a later worker.
The [VGA worker check](src/vga.rs) connects a device to guest MMIO and CRTC ports
on a CPU worker. A separate renderer worker consumes published immutable screens
while the CPU awaits input, then observes the final publication after CPU and device
teardown. The suspended service uses its local bus and mapped memory across the
await. VGA device and rasterization fixtures also run directly on the main thread;
no live device crosses workers.

From the repository root:

```sh
rustup toolchain install nightly-2026-10-20 --profile minimal --component rust-src
cargo install wasm-bindgen-cli --version 0.2.129 --locked
npm ci
CHROME_BINARY=/path/to/chrome npm test --workspace=relocade-browser-tests
```

The shared runner selects `CHROME_BINARY` and configures software WebGPU for
the renderer tests. ChromeDriver is not required.
CI uses the same entrypoint in [the browser job](../../.github/workflows/tests.yml).

For focused backend tests without the worker checks, use the `test:backends` script.
It accepts Cargo test filters, for example:

```sh
npm run test:backends --workspace=relocade-browser-tests -- --test memory
```

Cargo test filters can also be passed to the workspace's `npm test`, for example
`npm test --workspace=relocade-browser-tests -- --test memory`; the worker checks still run afterward.
For only the worker checks, run `npm run build --workspace=relocade-browser-tests`
followed by `npm run test:workers --workspace=relocade-browser-tests`.
`npm run dev --workspace=relocade-browser-tests` opens those checks for manual inspection after building them.
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

See the [worker guide](../../crates/machine/docs/workers.md) for message and
resource ownership. Add future cross-thread integration checks here so CPU,
renderer and device workers share this setup.
