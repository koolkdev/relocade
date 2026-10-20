# Worker execution

The intended interactive embedding uses one persistent async task owning the
guest environment, runtime and active service for the program's lifetime. In a
browser, this task can live in an application worker alongside message handlers.
The application owns that execution worker. The runtime also supports a separate
compiler worker which receives copied code and returns compiled modules.

```text
UI --input/control messages--> worker message handlers
                                      |
                               queues and wakeups
                                      |
                               execution task
                               owns DOS + runtime
```

For a blocking console read, the execution task awaits input after the guest
returns. A worker message handler receives input bytes and writes them through an
owned stdin handle, waking the same task. The handle accesses an independent
queue, so delivering input does not borrow the runtime. Browser stream handles
stay inside the worker; messages carry bytes across the thread boundary. A native
embedding can use channels and a task on the runtime's owning thread.

Worker creation, UI messaging and task lifetime belong to the embedding
application. The runtime owns execution and service completion; the web and
Wasmtime backends supply Wasm engine access. Generated guest calls remain
synchronous; the execution task awaits services after those calls return.

## Compiler workers

Wasmtime runtimes create a compiler thread sharing the backend's engine. Only
owned code snapshots and compiled modules cross that boundary; the store, guest
memories and host state stay on the execution thread. Host state need not be Send.
Dropping a runtime closes the queue without joining an active compilation.

Browser embeddings install `relocade_runtime_web::WebCompiler` with absolute URLs
for their own `wasm-bindgen --target web` bindings and Wasm binary:

```rust,ignore
runtime.set_compiler(Some(Box::new(WebCompiler::new(
    "https://example.test/app.js",
    "https://example.test/app_bg.wasm",
)?)));
```

The worker initializes a private compiler instance from the same build. It
discovers connected blocks, lowers them and calls `WebAssembly.compile`; the
resulting `WebAssembly.Module` returns by structured clone. The execution owner
instantiates it with private guest imports and validates the consumed code before
publishing any entries. This needs no shared guest memory or cross-origin
isolation. The application's CSP must permit the blob module worker and its
module and Wasm URLs. A backend without a compiler continues interpreting.
The supplied bundle's initialization, including any `#[wasm_bindgen(start)]`
function, must be worker-safe. Use a compiler-only entry point or keep UI startup
and compiler-worker creation in the execution embedding, outside module initialization.

`capture_connected_group` only copies up to 64 KiB of readable real-mode code.
`queue_group` is nonblocking and admits at most two outstanding jobs, deduplicated
by seed CS.base/EIP. A full worker queue declines work immediately.
`publish_ready_groups` checks completed jobs without waiting for compilation.
Pending snapshots do not watch pages: publication compares the instructions
actually consumed, so ordinary data writes do not invalidate pending work.
Published groups use the existing page-map bit and slow-path invalidation.

Jobs belong to the runtime, independently of each execution future. Replacing a
compiler keeps accepted jobs counted until their completion. Worker failures are
reported when collecting results, with no partial entries published.

## Task lifetime

The execution task has a lifetime independent of any individual UI request.
Dropping a UI waiter must not implicitly cancel the guest or its pending service.
An explicit stop command needs a control path that can wake the task while a
service is pending; merely checking commands after a read completes is insufficient.
Cancellation does not undo side effects already performed.

The task keeps the active service future alive through ordinary waits. This
ownership does not make dropping that future cancellation-safe; explicit
stop/reload behavior must follow the service API's cancellation contract.

## Event-loop scheduling

`run_async` divides its original instruction allowance into slices (65,536
instructions by default, configurable with `set_execution_slice`). It collects
ready results between slices and yields after each budget exit, including a
final short slice. The browser backend uses a MessageChannel task so compiler
messages and other host events can run; an already-ready future or a microtask
loop would starve them. The native backend yields to its async executor.

Internal slice exits do not replenish or return the caller's budget. Dropping a
run future at a slice preserves completed guest work and pending compiler jobs;
dropping during a host operation retains the separate host cancellation contract.
Synchronous `run` continues to expose its original budget and exits directly.
Budget checks remain at block boundaries; they provide
[no exact-stop or elapsed-time guarantee](../../x86/docs/host-integration.md#instruction-budgets).
