# Worker execution

The intended interactive embedding uses one persistent async task owning the
guest environment, runtime and active service for the program's lifetime. In a
browser, this task lives in a worker alongside message handlers. Worker support
is deferred; this note records its intended ownership and scheduling.

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

CPU-bound execution also needs finite instruction budgets and an actual yield
to the worker event loop between runs. Awaiting an already-ready future does not
yield, and repeatedly scheduling microtasks can starve message delivery. Budget
checks remain at block boundaries; they provide
[no exact-stop or elapsed-time guarantee](../../x86/docs/host-integration.md#instruction-budgets).
