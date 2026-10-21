# Compiler workers

`runtime::compiler_worker` owns guest-code compilation for native and browser
workers. Its one `run(endpoint, backend, lower)` implementation uses a typed
`RequestEndpoint::serve` loop supplied by `relocade-worker`:

```text
request bytes → lower(request) → LoweredModule { wasm, metadata }
              → WasmBackend::compile → Compilation { module, metadata }
```

The Rust lowering function returns `Result<Option<LoweredModule>>`; None means
unsupported input. Metadata belongs to that function's request format. The worker
has its own backend; the execution owner retains guest instances, CPU, memory,
host state, imports and dispatch. Completed modules are linked to those private
imports later.

`CompilerWorker<M>` is an alias for
`RequestWorker<Vec<u8>, Option<Compilation<M>>>`. The shared
[worker crate](../../worker/README.md) owns request admission, readiness, the
handler loop, message delivery, result futures and graceful shutdown. The
embedding chooses the outstanding-request limit; these examples use two.

## Native startup

The entry constructs its backend on the dedicated thread and calls the shared
compiler implementation. Wasmtime backends use the same engine, so execution
owners can link the returned modules. Only startup captures and messages cross
threads; worker-local state and the entry's future need not be Send.

```rust,ignore
let mut compiler = CompilerWorker::spawn("relocade-compiler", 2, |endpoint| async move {
    let backend = WasmtimeBackend::new()
        .map_err(|error| relocade_worker::Error::Failed(error.to_string()))?;
    compiler_worker::run(endpoint, backend, lower_request).await
})?;
```

## Browser startup

The embedding exports an entry from its worker-safe `wasm-bindgen --target web`
build. It constructs the browser backend and invokes the same Rust compiler:

```rust,ignore
#[wasm_bindgen]
pub async fn compiler_entry(_: JsValue, endpoint: relocade_worker::web::Endpoint,
                            _: JsValue) -> Result<(), JsValue> {
    compiler_worker::run(endpoint.into(), WebBackend::new(), lower_request)
        .await.map_err(|error| error.to_string().into())
}
let mut compiler = CompilerWorker::<WebAssembly::Module>::from_worker(
    relocade_worker::web::Worker::spawn(
        bindings_url, wasm_url, "compiler_entry", &JsValue::NULL,
    )?,
    2,
);
```

The module/metadata record derives `Message`, composing the worker's field
encodings. The compiler handler receives Rust values and returns Rust values.
Worker transport transfers owned request and metadata buffers and
structured-clones compiled modules.

URLs must be absolute. Startup loads a private Wasm instance; initialization must
be worker-safe. CSP must permit the bootstrap blob and binding and Wasm URLs.
Receiving replies requires an event-loop task opportunity on the execution
worker. Shared guest memory does not deliver module objects.

## Submission

Both platforms use the same client API and compiler entry:

```rust,ignore
let completion = compiler.submit(request_bytes)?.expect("capacity available");
let compilation = completion.await?;
```

An ordinary compilation failure affects only its request. Shutdown and owner
drop drain accepted work. Result futures outlive the owner; native result futures
are Send. Worker startup or entry failure completes pending requests with an error.

Workers do not choose hot entries, capture live code, validate snapshots or
publish dispatch entries. Those responsibilities belong to runtime JIT integration.
The request format is independent of any particular snapshot or block-group format.
