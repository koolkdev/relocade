# Asynchronous runtime

`wasm86-runtime` keeps guest execution and host memory ownership on one thread.
A worker receives owned requests, generates Wasm with `wasm86-codegen`, and
compiles Wasmtime modules. The execution owner installs completed modules between
Wasm invocations. It never waits for generation or compilation, including startup.
Compilation requests are explicit; automatic hotness tracking comes later.

The [Node.js/V8 adapter](v8/runtime.mjs) uses the same Rust generator and generated
interpreter/JIT semantics. Its JavaScript memory owner follows the same watch and
ticket protocol as `wasm86-code-cache`, keeping Wasm generator calls off the
execution thread. Mapping/alias metadata lives in `mappings.mjs`, memory mutation
and code lifetimes in `memory.mjs`, and scheduling/engine handles in
`runtime.mjs`. Wasmtime separates those responsibilities into `HostMemory`,
`CodeCache`, and `Runtime`/its compiler worker.

## Wasmtime

Create an engine supporting multi-memory, multi-value, tail calls and threads.
Create its store with `HostState::new(host_payload)`, initialize CPU, backing and
mapping memories, then adopt them with `HostMemory`. The linker supplies device
and descriptor callbacks from the [x86 host contract](../x86/docs/host-integration.md).
The runtime supplies memories, budget, dispatch, handoff and invalidation imports.

```rust,ignore
use wasm86_runtime::{HostMemory, HostState, Profile, Runtime, SliceExit};

let mut store = wasmtime::Store::new(&engine, HostState::new(devices));
// Allocate and initialize cpu, guest and table memories in this store.
let memory = HostMemory::new(&mut store, cpu, guest, table, Profile::Flat32);
let mut runtime = Runtime::new(store, linker, memory)?;
let job = runtime.request_block(0x1000, 16)?;
let slice = runtime.run_slice(4096)?;
match slice.exit {
    SliceExit::Starting => { /* do other host work while the interpreter compiles */ }
    SliceExit::Yielded => { /* process host events, then resume */ }
    SliceExit::Guest(exit) => { /* handle the ordinary x86 exit ABI */ }
    SliceExit::IncompatibleProfile => { /* rebuild with compatible assumptions */ }
    SliceExit::Unavailable => { /* inspect startup failure in compilations */ }
}
// slice.compilations reports installed, discarded and failed job IDs.
memory.write_backing(runtime.store_mut(), 0x2000, &[1, 2, 3])?;
```

`request_block(eip, instruction_limit)` protects and copies live fetch bytes before
returning. There is no caller-supplied snapshot. Watches cover pending and installed
code, including every backing alias. At most eight requests are pending; full
queues return `SubmitError::Full`. Blocks accept 1–256 instructions and copy at
most 15 bytes per instruction. An installed block remains usable while a replacement
compiles. Failed or invalidated requests release their watches and execution
continues in the interpreter.

Prepared modules use `register_code` and `install`, independently of the worker
queue. The loader first establishes that its artifact matches the guest image,
execution profile and runtime ABI. Registration protects every declared
CS-relative code range without copying or decoding guest bytes. It returns no
ticket if any dependency is unavailable; unlike JIT capture, it cannot accept a
partial prefix.

```rust,ignore
use wasm86_runtime::{CodeRange, CompiledEntry};

let ticket = runtime.register_code(0x1000,
    &[CodeRange { offset: 0x1000, bytes: 3 }]).unwrap();
// Load or compile the matching artifact off-thread while this ticket is live.
let prepared = CompiledEntry { module, entry: "block_1000".into() };
let installed = runtime.install(ticket, prepared)?;
```

`CompiledEntry` contains an already engine-compiled Wasmtime module and its entry
export; modules must have no start function or imported-memory initialization.
`install` instantiates and publishes it at an execution boundary using the same
operation as worker completions. A stale or already installed ticket returns
`false`. Installation failure releases its registration and retains any previously
installed block.
Use `cancel_code(ticket)` when abandoning an artifact. Tickets belong to one
runtime and describe code lifetimes, not worker jobs. The installation module
owns this common publication path; the worker owns generation/compilation only.

`HostMemory::write_backing` invalidates aliases before host or DMA writes, and
`remap` invalidates affected code before publishing new routing. Both accept
a `Store` or a Wasmtime `Caller`, so device callbacks use the same owner without
locks. The host payload is available through `store.data().host`. The interpreter's
watched write slow path calls this owner before changing memory; a compiled write
to a watched page hands off before current-instruction effects. Pending results
are accepted only while their unique tickets remain live. Dispatch requires no
code-byte scan or page-generation comparison.

Guest memories are private to the execution thread. Raw backing/table handles are
available for initialization and inspection; after adoption, mutations must follow
the owner's protocol. Host CPU edits, registration and compilation requests are
boundary operations. Device callbacks may invalidate/remap existing code, but
cannot create new watches while generated access proofs are live. Synchronous host callbacks
must return promptly if the embedding needs a wall-clock responsiveness guarantee.

All entries share a finite work budget. Ordinary instructions cost one unit; REP
charges each element and publishes resumable indices/count without retiring until
completion. A zero-count REP costs one unit. A failed REP chunk proof hands off to
the interpreter, which checks one element and re-enters decoding with EIP unchanged
if the repeat continues. This also handles writes to the REP instruction itself.

Each runtime uses one fixed profile. Entry selection checks profile and CS context;
a context change invalidates code. To change profiles, retain memory handles,
consume the runtime with `into_store()`, and construct a new memory owner/linker.
Guest state is retained and interpreter compilation starts asynchronously again.
Dropping the old owner disconnects its worker without joining compilation.
Wasmtime retains instantiated core modules in its Store arena: invalidation removes
dispatch entries and watches, but accepted replacements retain instance resources
until that Store is dropped. `into_store()` preserves this arena along with guest
state. Repeated installations can eventually reach Store limits and report an
instantiation failure; the interpreter remains available.

## Node.js / V8

Build the generator artifact before starting the application:

```sh
rustup target add wasm32-unknown-unknown
cargo build -p wasm86-codegen --target wasm32-unknown-unknown --release --locked
```

Use Node.js 24 and the ordinary host imports, including initialized private CPU,
guest and mapping memories. The runtime supplies budget, dispatch, handoff and
invalidation. A Node worker instantiates the generator and compiles all generated
Wasm; only completed `WebAssembly.Module` objects return to the execution thread.

```js
import { Runtime } from './crates/runtime/v8/runtime.mjs';

const runtime = new Runtime({
  generator: new URL('./target/wasm32-unknown-unknown/release/wasm86_codegen.wasm', import.meta.url),
  imports,
  profile: 'flat32',
});
const ticket = runtime.requestBlock(0x1000, 16);
const slice = await runtime.runSlice(4096);
// exit: starting, yielded, guest, incompatible_profile, or unavailable.
// Guest exits include the raw BigInt ABI value; compilations reports tickets.
runtime.memory.writeBacking(0x2000, [1, 2, 3]);
runtime.memory.remap(3, { kind: 'ram', backing: 0x2000, writable: true });
await runtime.close();
```

Prepared artifacts use the same registration and installation contract as
Wasmtime. Match the artifact to the guest image, profile and ABI, then register
all of its code ranges before asynchronous loading or engine compilation:

```js
const ticket = runtime.registerCode(0x1000, [{ offset: 0x1000, bytes: 3 }]);
// Obtain a matching WebAssembly.Module from an off-thread producer.
const installed = runtime.install(ticket, { module, entry: 'block_1000' });
```

`registerCode` returns `null` when complete dependencies cannot be protected.
`install` accepts an already compiled module with no start function or
imported-memory initialization. It returns `false` for a stale or already
installed ticket, and throws on instantiation/export failure after releasing
that registration. A failed replacement retains the previous installed block.
`cancelCode(ticket)` abandons a registration or invalidates an installed entry.
Worker completions call this same installation operation; stopping a worker
does not cancel a ticket already installed through another producer.

`requestBlock` returns `null` for a full queue and throws for an invalid request
or unavailable fetch bytes. `runSlice` yields to the event loop before installing
completions and entering Wasm. Overlapping slices are rejected. Device callbacks
may call `writeBacking` and `remap`; registration, capture, installation and close
require an execution boundary. `close` releases watches and terminates the worker.
To change profiles, close the runtime and construct another using the same
memories; discard the old memory owner. Shared memories and simultaneous owners
are rejected. Views are refreshed after host memory growth.

## Scope and validation

This stage uses host dispatch and one block per module. It deliberately leaves
hotness policy, dispatch tables, direct tail links and multi-block modules for
later work. Wasm traps remain host errors; expected guest faults remain ABI values.

```sh
cargo test -p wasm86-runtime --locked
cargo build -p wasm86-codegen --target wasm32-unknown-unknown --release --locked
cargo test -p wasm86-runtime --locked --test v8 -- --ignored
```

The V8 lane disables Liftoff, lazy Wasm compilation and tier-up. It checks worker
isolation, installation during guest progress, independently prepared modules,
watched aliases and remaps, device callbacks, and REP continuation. Shared
generated slice and write-guard behavior also has focused tests on both engines
in `wasm86-x86`.
