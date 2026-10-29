# Displayed application checks

The test-image application uses the public harness and shared image renderer.
It returns immediately after publishing its source, leaving graphics alive until
window close. Pixel assertions inspect the displayed window/canvas, followed by
normal shutdown and the retained result.

The DOS fixture delegates execution and drawing to `DosApplication`. A test-only
graphics gate lets the guest continue writing VRAM, reading status and accepting
keys while output is held. Browser checks cover latest-size demand, hidden output
without machine pause, retained output, a real device-loss callback, startup
cancellation and owner-side destruction. Native X11 checks use XTest keyboard
input and inspect the displayed DOS image before and after resizing.
The harness's private native fixture recreates a real surface through main and
configures/draws on its replacement using the same graphics owner and device.

The DPI check exercises winit's CSS-size/DPR fallback by hiding the optional
physical-box observer property: Chromium's CDP DPR emulation does not update that
observer's physical sizes. Actual cross-monitor DPI changes, driver-originated
surface loss, macOS/Metal and Windows still need platform runs.

```sh
cargo run -p relocade-app-tests
xvfb-run -a cargo test -p relocade-app-tests --test window -- --ignored
xvfb-run -a cargo test -p relocade-app --features window --lib -- --ignored
npm ci
node tests/app/web/build.mjs
CHROME_BINARY=/path/to/chrome node tests/app/web/test.mjs
```

Browser builds need the pinned threaded toolchain and wasm-bindgen CLI described
in the [browser test guide](../browser/README.md). The harness's worker bootstrap
and build flags are shared with those tests through [web tooling](../../tools/web).
