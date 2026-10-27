# DOS application

A basic DOS application with VGA visualization on a native window or browser
canvas. Both entrypoints run the same embedded program, which writes directly to
VGA text memory and exits. The display stays visible until the host closes.

```sh
cargo run -p relocade-dos-app
```

For the browser, install the [shared web tooling](../../tools/web/README.md), then:

```sh
npm ci
npm run dev --workspace=relocade-dos-app-web
```

The page initializes the bindings and starts the demonstration. Worker imports
remain side-effect free. `npm run build --workspace=relocade-dos-app-web` produces
`web/dist`; production hosting needs the isolation headers in the tooling guide.

## Embedding

`DosApplication` implements the existing application contract. It takes COM
program bytes, constructs DOS on the execution worker, and sends its VGA
`Display` to the shared `VgaRenderer`:

```rust,ignore
let control = DosApplication::new(program_bytes).open()?;
let result = control.await?;
```

`execution.rs` runs bounded CPU slices, observes cancellation and publishes
screens before waits, completion and at the display cadence. It discards the
DOS library's duplicate output buffers to prevent accumulation. Rendering,
textures, timers, window events and worker shutdown stay in the shared crates.
The window retains the final display after execution completes.

The browser `open(Uint8Array)` export uses the same supplied-byte path and returns
the host's `BrowserControl` for shutdown and completion. Keyboard integration,
terminal I/O, file selection and filesystem mounting are deferred.

## Verification

Native execution tests cover direct VGA writes, completion and cancellation.
The native window fixture and browser fixture call the production entrypoints
with literal program bytes. They check displayed pixels, resize, retained output
and worker cleanup without loading or input UI. CI runs both:

```sh
xvfb-run -a -s "-screen 0 1280x1024x24" cargo test -p relocade-dos-app --test window --locked -- --ignored
CHROME_BINARY=/path/to/chrome npm test --workspace=relocade-dos-app-web
```
