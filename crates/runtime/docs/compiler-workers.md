# Compiler workers

Enable the runtime's `compiler-worker` feature to use its shared compilation
handler with `web-thread-select`. The embedding owns the library's `Thread` and
uses its existing task API:

```rust,ignore
use relocade_runtime::compiler_worker::{self, Thread};

let compiler = Thread::new();
let task = compiler.run((), move |()| {
    compiler_worker::compile(create_backend, move || lower(request))
});
let module = task.await??.module;
```

`create_backend` returns `runtime::Result<B>` and runs on the compiler thread.
`lower` returns `runtime::Result<Vec<u8>>`; capture owned Rust requests in its
closure. The task closure must be Send, but the backend and async compilation
future created inside the worker need not be. Both platforms use the same handler.
Wasmtime backends share their existing engine, so returned modules can be linked
by another execution owner.

`CompiledModule<M>` implements the library's `Post` transport. Native modules
move as Rust values; browser modules use structured cloning through
`serde_wasm_bindgen::preserve`. Ordinary Rust requests need no serialization or
message derive. Errors are task-local string diagnostics and leave the thread
available for another job.

The execution owner instantiates the module with its private imports and retains
its callbacks, CPU, memory and dispatch table. Compiler tasks do not capture live
guest state, choose hot code or publish dispatch entries.

Keep the thread alive until its tasks finish. Reuse a long-lived thread and await
accepted work before dropping it. Browser bootstrap failure reporting and thread
allocation reclamation are tracked in [#439](https://github.com/koolkdev/relocade/issues/439).
Receiving compiled modules also requires an event-loop task opportunity on the
receiving browser thread.

## Browser build

Shared Rust host memory is distinct from shared guest memory. This feature needs
a threaded Rust build: atomics, bulk memory, mutable globals, rebuilt standard
library, shared imported memory, and the TLS exports used by wasm-bindgen.
The page must be cross-origin isolated with COOP/COEP headers.

The application must resolve `web-thread:wasm-shim` to its worker-safe bindings.
With our pinned versions, a small adapter passes the library's `module` argument
as wasm-bindgen's `module_or_path`. It provides no custom messaging protocol.
The executable configuration and adapter are in
[tests/compiler-worker](../../../tests/compiler-worker/README.md).

The runtime feature is optional. Ordinary runtime builds and browser tests keep
their existing stable toolchain and private Rust host memory.
