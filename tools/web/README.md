# Shared web tooling

Applications and browser tests use one threaded Wasm build, worker bootstrap and
Vite configuration. Install the JavaScript dependencies with `npm ci` from the
repository root; npm workspaces share the pinned Vite and Playwright versions.

The Rust toolchain is `nightly-2026-10-20` with `rust-src`, and the binding generator
is `wasm-bindgen-cli` 0.2.129. `build.mjs` rebuilds the standard library with
atomics, shared imported memory and per-thread TLS. It also exports the stack
pointer and permits table growth for typed runtime calls. The target directory
defaults to `target/browser` and honors `CARGO_TARGET_DIR`.

```js
import { buildBindings } from "./build.mjs";

buildBindings("relocade-browser-tests", "/absolute/path/to/pkg");
```

`buildBindings(packageName, directory, { release })` compiles a Cargo library and
generates web bindings in the supplied directory. `release` defaults to false.
`cargo(command, args, { runner })` exposes the same target and flags for checks,
Clippy and tests; test owners supply their Cargo runner explicitly.

`webConfig(root, bindings)` supplies the Vite worker aliases and cross-origin
isolation headers for development and preview servers. `worker-shim.mjs` adapts
web-thread's module argument to wasm-bindgen's initializer and preserves the
shared memory. Worker imports must not automatically launch an application.

Production hosting must also serve these headers:

```text
Cross-Origin-Opener-Policy: same-origin
Cross-Origin-Embedder-Policy: require-corp
```

`withBrowser(config, check)` starts Vite and Chrome, then calls
`check(page, baseUrl)`. It honors `CHROME_BINARY`, enables software WebGPU and
closes both resources when the check finishes. Test selection, assertions,
timeouts and output reporting stay with the owning suite. Pass `{ built: true }`
as the third argument to serve an existing production build through Vite preview.
See the [browser tests](../../tests/browser/README.md) for their entrypoints.
