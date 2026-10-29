# DOS viewport

`DosApplication::new(program_bytes)` implements `Application` for execution and
`WindowApplication` for drawing. It creates DOS on the execution worker and publishes its immutable capture
source. The graphics worker selects the current capture on host redraw, records VGA
decoding when the sequence changes, and draws the retained GPU image on resize/expose.
One submission contains decoding and output drawing; no CPU pixel readback is needed.

Run the echo example, or supply one COM/MZ executable:

```sh
cargo run -p relocade-dos-app
cargo run -p relocade-dos-app -- program.com
```

Keyboard input supports ASCII text, control characters, navigation/function keys and
BIOS modifier state. Unicode-to-codepage conversion, IME and keyboard-controller
emulation are outside this adapter. Program exit leaves the DOS session, VGA timing
and image alive. Close the window to release execution and graphics on their owners.

For the browser, install root npm dependencies and the shared [web toolchain](../../tools/web/README.md):

```sh
npm ci
npm --prefix apps/dos/web run build
npm --prefix apps/dos/web run dev
```

Open the printed URL; the default is the same echo example. `?program=/example.com`
fetches program bytes. Embeddings can call the generated `open(Uint8Array)` directly
and use its `stop()` and `finished` handles. The server must supply the COOP/COEP
headers configured by the shared Vite helper. Surface and canvas lifecycle remain
with the application harness, independently of DOS process exit.
