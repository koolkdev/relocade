# Workers

`relocade-worker` owns thread/worker startup, command and event delivery, and
completion. It has no runtime, compiler or Wasm-engine dependency. Raw workers
support independent commands and events. `RequestWorker<C, R>` adds a shared,
bounded request/reply owner for sequential services such as compilation.

## Requests

`RequestWorker` owns admission, outstanding requests, result futures and graceful
shutdown on both platforms. The caller chooses the capacity. `submit` returns
None when full; accepted requests start without polling their result future.
Capacity is released on delivery, even if the future was dropped or never polled.
Dropping a future does not cancel its request or shift subsequent replies.
An ordinary error fails only its request; worker failure completes pending results
with the terminal error. Results can outlive the submission owner.

```rust,ignore
let mut worker = RequestWorker::spawn("example", 2, |endpoint| async move {
    let state = initialize_worker()?;
    endpoint.serve(async |request| process(&state, request).await).await
})?;
let result = worker.submit(request)?.expect("capacity available");
worker.shutdown();
let response = result.await?;
worker.stopped().await?;
```

`RequestEndpoint<C, R>::serve` is the common worker-side implementation. It reports
readiness after local initialization, receives typed requests, awaits the handler,
and sends one result per request. A handler error leaves the worker running;
transport failure terminates the entry. The native entry's future and the state
created inside it need not be Send.

A browser export converts its raw endpoint with `endpoint.into()` and calls the
same Rust entry or `serve` implementation. The client uses
`RequestWorker::<C, R>::from_worker(web::Worker::spawn(...)?, limit)`.
Native transport delivers reply handles directly to the thread. Browser transport
retains replies locally and completes them in order as messages arrive. No request
IDs are needed for these sequential entries.

### Typed messages

`Message` describes values crossing this boundary. Native values need Send and
move directly. Browser values provide `encode` and `decode`: a `web::Packet`
contains a structured-clone value plus its owned buffers or ports to transfer.
Encoding is separate from the common request loop.

The worker crate provides browser encodings for owned byte vectors, pairs,
optional values, worker results/errors, strings, u32 values, JavaScript values and
compiled Wasm modules. Composite messages preserve nested transfer lists. Options
use explicit tags, so None differs from Some(null) and nested options retain their meaning.
Owned byte vectors are copied out of live Wasm memory before their buffer is
transferred; compiled modules are structured-cloned.

Derive `Message` on ordinary structs and enums to compose their field encodings:

```rust
#[derive(relocade_worker::Message)]
pub struct Compilation<M> {
    pub module: M,
    pub metadata: Vec<u8>,
}
```

The derive supports named, tuple and unit structs and enum variants. It preserves
generic bounds and requires each field type to be a message. It generates the browser conversion;
native values still move directly through the channel. There is no per-record
browser implementation to maintain. `relocade-worker-derive` owns code generation;
the worker crate owns field codecs and transfer-list composition.

Browser records use field order and enum tags use variant declaration order.
Workers must use the same message definitions; this is not a storage format or
a protocol for independently versioned programs. The derive uses the
`relocade_worker` crate name.

Raw workers remain useful for independent commands and unsolicited events.

## Raw messages

Native entries create state on their thread. That state and the entry's future
need not be Send; captured startup inputs and channel messages must be Send.
The thread runs the entry with `pollster`, without installing an I/O reactor.

```rust,ignore
let (mut worker, mut events) = native::Worker::spawn("example", |mut endpoint| async move {
    let state = initialize_worker()?;
    endpoint.ready();
    while let Some(command) = endpoint.recv().await {
        endpoint.send(handle(&state, command).await?)?;
    }
    Ok(())
})?;
worker.ready().await?;
worker.send(command)?;
let event = events.recv().await;
worker.shutdown();
worker.stopped().await?;
```

Browser startup loads a private instance of a worker-safe `wasm-bindgen --target
web` build. URLs are absolute. Its named async entry receives the initialized
bindings, an endpoint, and structured-cloned configuration:

```rust,ignore
#[wasm_bindgen]
pub async fn worker_entry(bindings: JsValue, mut endpoint: web::Endpoint,
                          configuration: JsValue) -> Result<(), JsValue> {
    endpoint.ready();
    while let Some(command) = endpoint.recv().await.map_err(|e| e.to_string())? {
        // Decode the consumer's command and send its event.
        endpoint.send(&command, &[]).map_err(|e| e.to_string())?;
    }
    Ok(())
}
let (worker, mut events) = web::Worker::spawn(bindings_url, wasm_url,
    "worker_entry", &configuration)?;
worker.ready().await?;
worker.send(&command, &[])?;
```

Browser messages retain JavaScript structured-clone types. `send(value, &[])`
copies ordinary buffers, shares shared memory, and clones compiled Wasm modules.
An explicit transfer list moves owned buffers or ports and detaches them from
the sender. Never transfer a live Wasm-memory buffer. Consumer protocols remain
responsible for decoding their commands and events.

`shutdown` and owner drop close command input. Already accepted messages remain
available; the entry sees None after draining them. They do not interrupt an
active operation. A CPU entry that waits for a service must keep a command pump
alive if it needs input or stop commands during that wait. Browser CPU entries
also need event-loop task opportunities during synchronous execution.

Readiness and completion futures can outlive the owner. Dropping those futures
does not stop the entry. Cancelling a receive does not consume an event.
Startup failure, native unwind or browser worker failure completes lifecycle
waiters with an error. Native event receivers close; `stopped()` reports the
failure. Browser receivers also report terminal failure after queued events.
Normal per-request errors belong in the consumer's reply protocol and need not
stop a worker. Returning before reporting readiness is a startup failure.

Browser initialization must not start a window UI. CSP must permit the worker
bootstrap blob and the module URLs. Native shutdown is cooperative; there is no
portable forced-termination operation.
