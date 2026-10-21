# Threaded runtime experiment

This example exercises a shared Rust heap on native threads and browser workers
using `web-thread-select`. It uses ordinary Rust channels and `Arc` state; worker
entry closures are `Send`, while async state constructed inside the worker can
contain `Rc`. The browser check also verifies independent thread-local storage
and that a channel moves the original allocation without serializing its bytes.

The compiler check lowers and compiles a module on one thread, then instantiates
it on another thread with that owner's private callbacks and non-Send state. The
same compiler thread accepts a valid request after a compilation failure. Rust
requests move in the task closure; browser module results use the library's
existing `Post` transport with `serde-wasm-bindgen::preserve`. No custom message
derive or transport is needed. Native modules cross as ordinary Send values.

The CPU check creates a runtime inside one persistent task. While guest code
loops, it reads a Rust command at an instruction-budget boundary and enters a
hook that awaits input. A Rust oneshot channel wakes that same handler; its
non-Send state and pending `run_async` future remain on the CPU thread. The task
returns the final CPU snapshot and completes before its thread owner is dropped.

This check uses shared Rust control while the worker is busy. A production CPU
loop must also periodically return to the browser event loop for JavaScript
events, including compiled-module delivery. This example does not choose that
scheduling policy or add CPU stop/reload cancellation.

Run the native check with the normal toolchain:

```sh
cargo test -p relocade-threaded-runtime --locked
```

The browser experiment has a separate threaded build. From this directory:

```sh
rustup toolchain install nightly-2026-10-20 --profile minimal --component rust-src
cargo install wasm-bindgen-cli --version 0.2.129 --locked
npm ci
npm run build
npx playwright install chromium
npm test
```

Set `CHROME_BINARY` to use an existing Chrome installation instead. `npm run dev`
opens the same checks for manual inspection. Vite supplies cross-origin isolation
headers and resolves the library's `web-thread:wasm-shim` import through a small
adapter to our generated bindings. Neither the normal workspace build nor
ordinary browser tests require the threaded toolchain.

The thread owner stays alive until its task completes. Closing the Rust command
channel lets the task finish normally; dropping a browser thread is not a
replacement for graceful task shutdown. A shared host heap does not make browser
API handles thread-safe, and is separate from the guest's shared linear memory.

The experiment does not establish complete thread-stack/TLS reclamation or
startup-failure recovery. The published browser library closes workers without a
Rust thread-destruction call, and does not reject readiness on bootstrap failure.
Those lifecycle gaps need resolving before using it for replaceable production
workers; they do not require another lifecycle framework in this example.
