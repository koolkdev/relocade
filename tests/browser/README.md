# Browser tests

All browser tests use the same threaded Rust build: shared host memory, atomics,
and per-thread TLS. The linker also exports Rust's stack pointer and permits its
function table to grow, so typed runtime calls can stay entirely inside Wasm.
Arguments and scalar results pass directly; aggregate results follow Rust's C
return convention through generated Wasm glue. `npm test` runs the wasm-bindings,
wasm-runtime, machine, VGA and renderer tests with wasm-bindgen-test, then the worker
integration checks. Both stages run
through Vite and Chrome, sharing the toolchain, flags, Cargo target directory
and worker bootstrap used by browser applications.

The worker checks cover Rust channel wakeups after a task returns Pending,
host timer delivery while an execution worker stays continuously runnable,
independent TLS and worker-local Rc state, shared-memory messages that preserve
later growth and writes after the allocating worker exits, and compiled-module
handoff between execution owners. The [CPU worker example](src/cpus.rs) retains
caller access to mapped memory while a worker awaits Rust input, resumes guest
execution, and imports the retained allocation into a later worker.
The [VGA fixture](src/vga.rs) registers VGA alongside the CPU. Guest code uses
MMIO and CRTC ports, polls retrace, and awaits input while immutable text captures
reach a separate rasterizing worker. It observes final writes after CPU completion,
and retained frames stay readable after the session shuts down.

The [graphics fixture](src/graphics.rs) explicitly assembles a `Thread`, a
single-slot Rust request channel and a local wgpu target. It demonstrates one
hosting arrangement, with no rendering worker or request protocol in VGA.
A barrier holds conversion while the guest writes VRAM and reads status.
Request bursts coalesce; pausing requests stops conversion; resumed requests
select recent captures. The same worker rasterizes and draws through wgpu,
including GPU readback, resizing and browser OffscreenCanvas presentation.
Closing VGA leaves the surface alive and the last capture renderable; the fixture
app closes its request channel separately to end presentation and drop resources.

The [GPU text fixture](src/graphics/gpu.rs) uses the same guest with explicitly
requested GPU decoding on a separate worker. The fixture records VGA decoding
and image drawing into one command encoder, owns its output pass, and submits
the batch. It checks bounded requests, guest progress while graphics is held,
and rendering retained captures after execution ends, without owning a surface.

The [DOS fixture](src/dos.rs) explicitly assembles its own rendering worker and
reuses the concrete GPU target. It checks direct VGA output, DOS input/echo,
final writes at process exit, paused redraws and a later launch in the same DOS
session. Presentation continues from the retained capture after DOS shuts down;
the fixture closes the host worker separately. DOS and BIOS service semantics
remain in their native suites.

The [execution-worker fixture](src/runner.rs) receives host timer notifications
during busy CPU execution and suspended services, stops either case, and checks
that local CPU state is destroyed before shutdown completes.

From this directory:

```sh
rustup toolchain install nightly-2026-10-20 --profile minimal --component rust-src
cargo install wasm-bindgen-cli --version 0.2.129 --locked
npm ci
CHROME_BINARY=/path/to/chrome npm test
```

The shared runner selects `CHROME_BINARY` and configures software WebGPU for
the renderer tests. WebGPU uses the OpenGL ES adapter so it can share canvas
images with the SwiftShader compositor. ChromeDriver is not required.
CI uses the same entrypoint in [the browser job](../../.github/workflows/tests.yml).

For focused backend tests without the worker checks, use `npm run test:backends`.
It accepts Cargo test filters, for example:

```sh
npm run test:backends -- --test memory
```

Cargo test filters can also be passed to `npm test`, for example
`npm test -- --test memory`; the worker checks still run afterward.
For only the worker checks, run `npm run build && npm run test:workers`.
`npm run dev` opens those checks for manual inspection after building them.
The build script defaults to `target/browser` and honors `CARGO_TARGET_DIR`.
Worker fixtures use Cargo's test profile, including optimization and debug
assertions, just like the native integration tests.
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
