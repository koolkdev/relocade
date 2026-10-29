# Threaded browser tooling

Run `npm ci` at the repository root. Npm workspaces share Vite and Playwright.
`buildBindings(packageName, outputDirectory)` builds one Rust cdylib with the
shared-memory/TLS flags and the test profile, then emits browser bindings.
`cargo(command, args, { runner })` uses the same toolchain and flags for tests.

`webConfig(root, bindings)` supplies worker aliases and isolation headers for
Vite development and preview. `worker-shim.mjs` initializes each worker with the
already compiled module and shared memory. `withBrowser(options, check)` owns
the test server and Chrome lifetime, using a common software WebGPU/compositor
configuration. Set `CHROME_BINARY` to choose the test browser.
