# Compilation

Both Wasm backends compile through a persistent `CompilerWorker`. The worker
starts on the first request. Wasmtime backends share it with their process-wide
engine; browser backends on the same JavaScript thread share one compilation
owner. Constructing a backend for memory access alone does not start a worker.

`WasmBackend::compile_with` takes an owned lowering closure. The runtime uses it
to generate its dispatcher and hooks on the worker:

```rust,ignore
let module = backend.compile_with(move || {
    let compiler = Compiler::new(target.profile).with_cpu_model(target.cpu_model);
    Ok(compiler.compile_dispatcher()?.bytes)
}).await?;
```

`WasmBackend::compile(bytes)` copies existing bytes into an owned request for
the same worker. This handles embedded interpreters and installed modules.
Module-cache lookup stays with the receiving owner, so cache hits avoid worker
messages. Existing static-asset identity and engine compatibility rules apply.

The shared worker runs both lowering and engine compilation. Backends supply a
small `WasmCompiler` implementation containing only engine compilation state.
Its future is created on the worker and need not be Send. Compiled modules return
through `Message`; failure diagnostics return as strings. Failed requests leave
the worker available for later work.

The execution owner binds imports, instantiates the module and publishes dispatch
entries after awaiting the result. CPU state, memories, callbacks and environment
state stay there. Hook generation sends only owned import names to the worker.
The runtime's existing preparation and installation APIs keep their signatures.

See [worker tasks and messages](workers.md) for transport and lifetime behavior.
