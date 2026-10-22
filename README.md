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
loads, FLAGS stack transfers, near/far transfers and software interrupts with
interrupt returns. External interrupt delivery and mode transitions remain outside
that subset. The project is under development; the crates are not published.

| Crate | Responsibility |
| --- | --- |
| [relocade-x86](crates/x86/src/lib.rs) | x86 decoding, shared semantics, CPU state and generated execution entries. |
| [relocade-interpreter](crates/interpreter/src/lib.rs) | Build-generated interpreter Wasm shared by native and web runtimes. |
| [relocade-compiler](crates/compiler/src/lib.rs) | Guest-independent typed expressions and structured control lowered to Wasm. |
| [relocade-machine](crates/machine/src/lib.rs) | Machine resources, CPUs, mapped memory, runtime execution and worker integration. |
| [relocade-wasm-runtime](crates/wasm-runtime/src/lib.rs) | Native and browser Wasm compilation, linking, memory and host bindings. |
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

The [future worker design](crates/machine/docs/worker-execution.md) describes
execution-task ownership, UI messaging and scheduling for an interactive embedding.

## Configure a protected user machine

`Machine` owns the address space and creates independently executable CPUs.
`MachineConfig` describes shared backing, mappings and descriptor bytes;
`CpuConfig` describes one CPU's target and initial architectural state.
Physical maps support Real16, and paged maps support protected profiles.

```rust
use relocade_machine::{CpuConfig, DescriptorMemory, Machine, MachineConfig, PagedMemoryMap};
use relocade_x86::{Compiler, CpuState, DescriptorTables, SegmentProfile};

let mut pages = PagedMemoryMap::default();
pages.map(0x400000..=0x40ffff, 0, true)?;
let descriptors = DescriptorMemory::new(DescriptorTables::default(), None)?;
let cpu_config = CpuConfig::new(
    Compiler::new(SegmentProfile::Flat32).target(), CpuState::default(),
)?.with_descriptor_memory(&descriptors)?;
let machine_config = MachineConfig::new(0x10000, pages)?
    .with_descriptor_memory(descriptors)?;
let machine = Machine::new(&mut backend, machine_config)?;
let mut cpu = machine.create_cpu(backend, cpu_config).await?;
```

`cpu.context()` lends CPU, memory and descriptor views. Host calls use the same
views through `HostCall`; descriptor edits leave loaded segment caches intact.
Each CPU keeps its configured execution profile. `CpuView::set_segment` and
guest segment resolution panic if a proposed cache violates that profile.
Profile switching is not supported. GDTR, IDTR and the loaded LDTR/TR caches are
direct `CpuState` fields. Descriptor tables occupy dedicated Wasm memory, so they
reserve no guest addresses. Page mappings are host-managed; guest CR3 walking
and privilege transitions remain outside this user-mode runtime.

CPUs retain their machine while owning private registers, loaded caches, dispatch
entries and pending host calls. Shared-memory access is not serialized. The
embedding coordinates mapping changes; remapping invalidates generated entries
on each CPU before its next execution, while host hooks survive.
See [shared memory](crates/machine/docs/shared-memory.md) for multiple CPUs,
code snapshot versions and worker transport.

The `runtime` module owns code preparation, dispatch and execution. Machine-wide
runtime state reuses generated dispatcher bytes by execution target; bound Wasm
instances stay with each CPU. The generic Wasm backend remains a separate crate.
For custom embeddings, `Machine::bind_cpu` allocates a private CPU context and
returns `CpuBindings`. Its `imports` supplies memory bindings and `with_host_call`
lends published CPU, mapped memory, descriptors and environment state to a
callback. Raw port and MMIO callbacks do not provide that publication boundary.

## Build and test

Cargo generates four interpreter modules on the build host, one for each execution
profile. Each exports `step` and `run`. `relocade-interpreter::module(profile)`
returns their embedded Wasm bytes, and `Machine::create_cpu` uses them in both native and
web builds. Starting a guest does not regenerate the interpreter. The backend
still compiles the portable Wasm to native code when loading it.

`backend.compiler()` returns an independent `WasmCompiler` handle. Its
`compile(bytes)` method compiles any module without borrowing the backend's live
instances or host state. Runtime startup retains only the four interpreter assets
in a shared `ModuleCache`, using that same compilation method on a cache miss.
Dispatcher and guest modules go directly to `compile`.
Backends provide optional cache storage through `module_cache()` and share it only
with compatible engines. Native backends share a Wasmtime engine and cache within
the process; browser backends share a cache within the same worker. The common
cache coordinates concurrent requests and retries failures. Each CPU keeps its own instances, register storage and host state.

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
