# relocade

Rust components for running 16- and 32-bit x86 programs through WebAssembly,
on native hosts and in browsers. The workspace includes an x86 compiler and
interpreter, an asynchronous machine runtime, a DOS environment, and VGA text
output with GPU rendering.

Relocade is a set of embedding libraries. Applications supply their own UI,
event loop, file persistence and display surface. The project is under active
development, and the crates are not yet published.

## What works today

- **x86 execution:** snapshot block compilation and a generated interpreter share
  instruction semantics. The supported instruction subset includes 16/32-bit
  integer operations, x87 floating point, MMX, SSE and SSE2, with real-mode and
  protected-mode execution profiles.
- **DOS programs:** load a real-mode COM or MZ executable with command-line and
  environment data. A persistent session provides console I/O, conventional-memory
  allocation, mounted in-memory filesystems, directories and file handles.
- **BIOS and text display:** keyboard input, basic BIOS video services and clock
  reads, plus an 80×25 VGA text display with CP437 glyphs, colors, blinking, pages
  and a hardware cursor. Guest memory and port accesses reach the same VGA device
  used by BIOS and DOS output.
- **Native and browser integration:** Wasmtime on native targets and browser
  WebAssembly on the web, asynchronous host services, worker tasks and shared
  memory. Text frames can be presented through `wgpu` on native surfaces or
  browser WebGPU.

The CPU compatibility target is **Pentium 4 user-mode instruction-set parity**;
this is a goal, not a claim of complete coverage. Privilege and mode transitions
and external interrupt delivery remain outside the current execution subset.
DOS and BIOS services are also partial: the current display is text-only, and
the embedding must advance the BIOS clock. See the
[instruction definitions](crates/x86/src/instruction/definitions),
[DOS API documentation](crates/dos/src/lib.rs), and
[VGA guide](crates/vga/README.md) for the implemented scope.

## Try it

Use a current stable Rust toolchain for native builds; CI tracks stable. From a
checkout, run the bundled [Hello, DOS example](crates/dos/examples/hello.rs):

```sh
git clone https://github.com/koolkdev/relocade.git
cd relocade
cargo run -p relocade-dos --example hello --locked
```

It runs a small COM program that prints `Hello, DOS!` through INT 21h, exits with
status 0, and copies the captured guest output to the host terminal. No DOS
installation, assembler or GPU is needed for this example.

The first build generates four interpreter Wasm modules, one per execution
profile, and can take a while. Later builds reuse them when their inputs are
unchanged. The runtime loads these assets when creating a CPU; it does not
regenerate the interpreter for each guest.

The example uses the same session API as a larger embedding:

```rust,ignore
let mut dos = relocade_dos::Dos::new().await?;
dos.launch_com(&program, Default::default())?;
let exit = dos.cpu().run(100).await;
let status = dos.process().unwrap().status();
```

Use `launch_mz` for MZ executables and `LoadOptions` for command-line and
environment data. `Dos::mount` supplies a filesystem at a drive letter;
`stdin`, `stdout`, `stderr` and `keyboard` provide host I/O. A session retains its
mounts and console across program launches. The
[DOS crate](crates/dos/src/lib.rs) documents these interfaces and supported services.

`Cpu::run` awaits pending host services such as console input. Interactive hosts
need finite instruction budgets and explicit event-loop yields between runs;
budgets are checked at block boundaries and do not guarantee an exact stop or
elapsed time. See [execution and scheduling](crates/machine/docs/worker-execution.md).

## How the pieces fit

Choose the layer your application needs. `relocade-dos` assembles a DOS session
from the machine, BIOS, filesystem and VGA components. `relocade-machine` also
supports custom environments with their own memory maps, devices and host hooks.
The compiler, Wasm runtime and bindings can be used independently of DOS.

| Crate | Responsibility |
| --- | --- |
| [relocade-dos](crates/dos/src/lib.rs) | COM/MZ loading, process lifetime, DOS services and console/file I/O. |
| [relocade-bios](crates/bios/src/lib.rs) | Real-mode keyboard, text-video and clock services. |
| [relocade-vfs](crates/vfs/src/lib.rs) | In-memory files, directories, handles and drive mounts. |
| [relocade-vga](crates/vga/README.md) | VGA text memory and registers, display snapshots and RGBA rasterization. |
| [relocade-renderer](crates/renderer/README.md) | Present RGBA frames through `wgpu` on native and browser hosts. |
| [relocade-machine](crates/machine/src/lib.rs) | CPU and memory access, device buses, dispatch, async host hooks and worker integration. |
| [relocade-x86](crates/x86/src/lib.rs) | Instruction decoding, shared semantics, CPU state and x86-to-Wasm compilation. |
| [relocade-interpreter](crates/interpreter/src/lib.rs) | Interpreter Wasm generated during the Cargo build and embedded for both runtimes. |
| [relocade-compiler](crates/compiler/src/lib.rs) | Guest-independent typed expressions and structured control lowered to Wasm. |
| [relocade-wasm-runtime](crates/wasm-runtime/src/lib.rs) | Native/browser Wasm compilation, linking, memory and host bindings. |
| [relocade-wasm-bindings](crates/wasm-bindings/README.md) | Portable Rust/Wasm value layouts and direct browser calls and callbacks. |

[relocade-derive](crates/derive/src/lib.rs) supplies the ABI and compiler-value
derive macros; [relocade-test-support](crates/test-support/src/lib.rs) supplies
internal Wasmtime test helpers.

### CPU execution

`relocade-x86` generates both snapshot blocks, which decode supplied bytes at
compilation time, and an interpreter, which decodes guest memory at runtime.
Both frontends use the same instruction definitions and semantics.
`Cpu` starts with the interpreter and can dispatch to modules installed by the
embedding. The embedding owns snapshot validity and invalidation.

For custom CPU embeddings, start with the
[machine API](crates/machine/src/lib.rs). Lower-level generated Wasm uses the
[host integration contract](crates/x86/docs/host-integration.md), including imported
memory, dispatch, fault exits and segment resolution. The
[physical-memory contract](crates/x86/docs/physical-memory.md) describes Real16
memory maps and host transfers. Both contracts are included in x86 rustdoc.

### Workers and display

Worker support is implemented behind the machine/runtime `worker` feature. The
[worker guide](crates/machine/docs/workers.md) covers task creation and resource
handoff; the [shared-memory guide](crates/machine/docs/shared-memory.md) covers
retained memory access.

In a browser, construct and run DOS sessions on a worker. VGA device access also
belongs on workers; a renderer worker can retain `dos.vga()` and sample the
display while the CPU awaits input. `TextRenderer` converts snapshots to RGBA,
and `relocade-renderer` draws those pixels to an output texture. The host owns
canvas/window setup, surface presentation and frame scheduling. See the
[VGA threading contract](crates/vga/README.md) and
[renderer guide](crates/renderer/README.md).

Browser builds need the threaded Rust configuration, worker bindings and
cross-origin isolation described in the [browser setup guide](tests/browser/README.md).
That guide contains the pinned nightly and Wasm tooling versions, npm commands
and Chrome configuration used by the browser integration suite.

## Development

Build API documentation from the repository root:

```sh
cargo doc --workspace --no-deps --locked
```

Open `target/doc/relocade_dos/index.html` for the DOS API or
`target/doc/relocade_machine/index.html` for custom machine integration.

For a focused native check of COM loading, output and termination:

```sh
cargo test -p relocade-dos --test com --locked
cargo fmt --all -- --check
```

[CONTRIBUTING.md](CONTRIBUTING.md) covers instruction development, test ownership,
Clippy and full workspace checks. CI runs the full native suite and browser
integration tests; DOS and VFS behavior tests stay native. The
[browser suite](tests/browser/README.md) documents full and focused web runs.

## License

[MIT](LICENSE).
