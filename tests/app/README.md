# Displayed application checks

The test-image application uses the public harness and shared image renderer.
It returns immediately after publishing its source, leaving graphics alive until
window close. Pixel assertions inspect the displayed window/canvas, followed by
normal shutdown and the retained result.

```sh
cargo run -p relocade-app-tests
xvfb-run -a cargo test -p relocade-app-tests --test window -- --ignored
npm ci
node tests/app/web/build.mjs
CHROME_BINARY=/path/to/chrome node tests/app/web/test.mjs
```

Browser builds need the pinned threaded toolchain and wasm-bindgen CLI described
in the [browser test guide](../browser/README.md). The harness's worker bootstrap
and build flags are shared with those tests through [web tooling](../../tools/web).
