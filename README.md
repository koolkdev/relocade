# relocade

Rust components for executing x86 code in WebAssembly.

`relocade-x86` generates snapshot blocks and an interpreter that share instruction
semantics. Blocks decode supplied bytes at compilation time; the generated
interpreter decodes guest memory at runtime. The embedding host supplies memory,
dispatch and segment descriptor resolution.

The project aims for Pentium 4 (P4) user-mode instruction-set parity.
Development prioritizes missing P4 instructions and architectural behavior.

The implementation currently covers part of the 16/32-bit protected-mode user
instruction set and ordinary real-mode execution. Real mode supports segment
loads, FLAGS stack transfers, near/far transfers, software interrupts, device IRQs
and interrupt returns. Full real-mode execution delivers divide errors, BOUND
violations and invalid opcodes through the guest IVT. Other exception delivery
and mode transitions remain outside that subset. The project is under development;
the crates are not published.

| Crate | Responsibility |
| --- | --- |
| [relocade-frontend](crates/frontend/README.md) | Shared winit input and wgpu display contracts for application integration. |
| [relocade-app](crates/app/README.md) | Main-thread winit application startup on native platforms and the browser. |
| [relocade-x86](crates/x86/src/lib.rs) | x86 decoding, shared semantics, CPU state and generated execution entries. |
| [relocade-cache](crates/cache/src/lib.rs) | Persistent byte artifacts with native and browser storage. Callers define keys and formats. |
| [relocade-interpreter](crates/interpreter/src/lib.rs) | Versioned interpreter Wasm generated on demand and cached on native and web runtimes. |
| [relocade-compiler](crates/compiler/src/lib.rs) | Guest-independent typed expressions and structured control lowered to Wasm. |
| [relocade-wasm-runtime](crates/wasm-runtime/src/lib.rs) | Native and browser Wasm compilation, linking, memory and host bindings. |
| [relocade-workers](crates/workers/src/lib.rs) | Native and browser workers, resource transfer and pools using caller-supplied threads. |
| [relocade-machine](crates/machine/src/lib.rs) | CPU views, execution, dispatch, hooks and worker tasks. |
| [relocade-pc](crates/pc/README.md) | PC hardware wiring, memory map and BIOS-initialized CPU. |
| [relocade-vga](crates/vga/README.md) | VGA text registers, fixed timing, immutable captures and programmable GPU text decoding. |
| [relocade-dos](crates/dos/src/lib.rs) | DOS allocator, filesystems, console and process services on a PC. |
| [relocade-renderer](crates/renderer/README.md) | Generic RGBA pixel presentation through wgpu. |
| [relocade-test-support](crates/test-support/src/lib.rs) | Internal Wasmtime test support. |

## Generate a block

```rust
use relocade_x86::compile_block_from_bytes;

fn main() -> Result<(), relocade_x86::BlockError> {
    // MOV EAX, 42 under flat 32-bit assumptions.
    let module = compile_block_from_bytes(0x1000, &[0xb8, 42, 0, 0, 0], 1)?;
    assert_eq!(module.entry, "block_1000");
    // Instantiate module.bytes with the host imports described below.
    Ok(())
}
```

For an executable embedding, follow the
[host integration contract](crates/x86/docs/host-integration.md): imported memories,
dispatch and fault exits, segment resolution, and entry validity. Real16 uses
the [physical-memory contract](crates/x86/docs/physical-memory.md) for host transfers.
Both contracts are included in the x86 crate's generated API documentation.

The [shared clock guide](crates/machine/docs/clock.md) describes elapsed time,
deadline waits and timer delivery through Rust channels.
The [execution guide](crates/machine/docs/worker-execution.md) describes the common
CPU/device runner, execution ownership and input delivery for an interactive embedding.

## Build and test

`Cpu::new` prepares only the requested interpreter profile. The module exports
both `step` and `run`; CPU features remain live execution-context inputs. Generated
Wasm is cached in the user's platform cache directory under `relocade/artifacts`
on native targets and in origin-scoped CacheStorage in browsers. The cache key
includes the execution profile and a fingerprint of the generator, ABI sources,
manifests and dependency lockfile, including uncommitted source changes. Cargo's
build script computes this fingerprint without generating interpreter Wasm.

A missing, unavailable or damaged native cache regenerates the artifact. Failed
writes do not prevent startup, and generation failures remain visible to the
caller. Native Wasmtime compilation continues to use its own disk cache. Cache
entries are disposable; automatic eviction of old interpreter revisions is not
currently provided.

`relocade-cache::ArtifactCache` owns the reusable storage and read-through policy:

```rust
let cache = relocade_cache::ArtifactCache::open().await;
let bytes = cache.load_or_generate(&artifact_key, generate).await?;
```

Keys are opaque strings shared across consumers: include a consumer namespace,
format/generator version and every generation input. Artifact bytes can contain
Wasm plus serialized metadata; the consumer owns that format. The interpreter
layer defines its revision/profile keys and uses this common cache. Future JIT
artifacts can use the same storage while keeping snapshot identity, entry metadata
and runtime validity with their execution owner. JIT persistence is not enabled yet.
Concurrent misses may generate independently; in-memory coordination of compiled
Wasm modules remains in `relocade-wasm-runtime::ModuleCache`.

Default CPU preparation generates a missing artifact on its calling thread.
Embeddings that own compilation workers can prepare the persistent artifact first:

```rust
let bytes = relocade_interpreter::load_or_generate(profile, || async {
    relocade_x86::build_interpreter(profile)?
        .compile_with(&executor)
        .await
}).await?;
```

The callback chooses execution resources and only runs on a cache miss. Retain and
drain the worker pool independently of cancellable CPU startup. The interpreter
cache itself does not own compiler workers or an engine.

`relocade_x86::build_interpreter(profile)` constructs a `Program` containing both
entry exports. The caller compiles that program once and selects an export when
invoking it. Applications can pass their own executor to `Program::compile_with`.
With the `relocade-workers/compiler` feature, an existing set of workers can be used
as follows:

```rust
let program = relocade_x86::build_interpreter(profile)?;
let (pool, executor) = relocade_workers::Pool::new(threads)?;
let ((), bytes) = futures_util::future::join(pool.run(), async move {
    program.compile_with(&executor).await
}).await;
let bytes = bytes?;
```

The caller supplies the threads and polls the pool driver. Cloned handles share
its concurrency limit. Drop all handles and await the driver to drain accepted
work, including after cancelling a compilation. Synchronous `compile()` remains
available without an executor.

`backend.compiler()` returns an independent `WasmCompiler` handle. Its
`compile(bytes)` method compiles any module without borrowing the backend's live
instances or host state. CPU startup retains interpreter modules by their versioned profile keys
in a shared `ModuleCache`, using that same compilation method on a cache miss.
Dispatcher and guest modules go directly to `compile`.
Backends provide optional cache storage through `module_cache()` and share it only
with compatible engines. Native backends share a Wasmtime engine and cache within
the process; browser backends share a cache within the same worker. The common
cache coordinates concurrent requests and retries failures. Each runtime keeps
its own instances, memory and host state.

Wasmtime's default disk cache also reuses compiled code across processes; its
standard cache configuration controls the location and eviction. If cache
configuration cannot be loaded, the backend reports the problem and continues
without disk caching.

Compiler and x86 correctness tests execute Wasm in Wasmtime. Browser integration
tests exercise the web runtime in Chrome; see [CONTRIBUTING.md](CONTRIBUTING.md).

```sh
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all -- --check
cargo doc --workspace --no-deps --locked
```

Open `target/doc/relocade_x86/index.html` or
`target/doc/relocade_compiler/index.html` for API contracts and examples.
[CONTRIBUTING.md](CONTRIBUTING.md) covers adding instructions, writing tests and
running the browser integration suite. Exact supported encodings live with
[their instruction definitions](crates/x86/src/instruction/definitions).
