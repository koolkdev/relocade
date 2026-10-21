# Compiler worker checks

This fixture calls `runtime::compiler_worker` on native threads and browser
workers. It verifies that lowering runs off the caller thread, owned Rust input
keeps its allocation, lowering/compilation failures leave the worker usable, and
another execution thread can link the returned module with private `Rc` state
and host callbacks.

Run the native check from the workspace root:

```sh
cargo test -p relocade-compiler-worker-tests --locked
```

Run the threaded browser check from this directory:

```sh
rustup toolchain install nightly-2026-10-20 --profile minimal --component rust-src
cargo install wasm-bindgen-cli --version 0.2.129 --locked
npm ci
npm run build
npx playwright install chromium
npm test
```

Set `CHROME_BINARY` to use an installed Chrome instead. `npm run dev` opens the
same fixture for manual inspection. The build script keeps the thread flags and
target directory separate from ordinary browser builds; Vite supplies the
required headers and binding alias. CI runs this fixture in its own job.

See the [compiler worker guide](../../crates/runtime/docs/compiler-workers.md)
for the production API and ownership requirements.
