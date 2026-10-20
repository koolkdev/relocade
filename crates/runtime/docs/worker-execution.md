# Worker execution

An interactive embedding gives each guest CPU a dedicated execution thread or
Web Worker. For DOS, one worker owns the runtime, DOS services and devices. Create
these objects inside that worker: host state and Wasm engine handles do not need
to be Send. One persistent async task owns the active program and service future.

```text
UI --input/control messages--> CPU worker
                               | message handlers feed queues and wakeups
                               | execution task owns DOS + runtime
                               |
UI / renderer <---shared guest memory
```

For a blocking console read, the execution task awaits input after the guest
returns. A worker message handler receives input bytes and writes them through an
owned stdin handle, waking the same task. The handle accesses an independent
queue, so delivering input does not borrow the runtime. Browser stream handles
stay inside the worker; messages carry bytes across the thread boundary. A native
embedding can use channels and a task on the runtime's owning thread.

Worker creation, UI messaging, execution intervals and task lifetime belong to
the embedding application. The runtime owns execution and service completion;
the web and Wasmtime backends supply Wasm engine access. Generated guest calls
remain synchronous; the execution task awaits services after those calls return.
The renderer samples shared memory independently; CPU execution does not wait
for a frame to be presented.

Additional guest CPUs would each own private execution state while sharing
machine memory and devices. Shared memory alone does not provide multicore
device synchronization or guest instruction atomicity.

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

The browser worker has its own event loop. A long synchronous Wasm call keeps
that loop busy even though the UI thread remains responsive. Input/control
messages and compiled-module deliveries cannot run until it gets a task
opportunity. Awaiting an already-ready future does not yield, and repeatedly
scheduling Promise microtasks can starve message delivery.

The worker's execution loop chooses an instruction allowance per run and calls
`relocade_runtime_web::yield_to_event_loop` between runs. This uses a persistent
MessageChannel, avoiding the nested-timer clamp and working without a Window.
For example, after creating a runtime inside the worker:

```rust,ignore
loop {
    // Handle commands while guest execution is stopped.
    let exit = runtime.run_async(instructions_per_run).await?;
    if exit != ExecutionExit::BudgetExhausted {
        return Ok(exit);
    }
    yield_to_event_loop().await?;
}
```

`Runtime::run_async` retains its caller-supplied budget across deferred services;
it does not add internal intervals or scheduler yields. `WasmBackend` has no
scheduling policy. A native execution thread can receive commands at the same
boundaries without yielding to an async executor after each run. When a service
awaits input, its channel or input task must be able to wake it independently of
the guest loop.

Choose an execution interval using measured throughput and input latency. Budget
checks stay at existing block boundaries and provide
[no exact-stop or elapsed-time guarantee](../../x86/docs/host-integration.md#instruction-budgets).
A long straight-line run or REP can overshoot. The browser helper provides a task
opportunity; it does not wait for any particular input or compiler completion.

Background compilers operate independently. Their completed modules are received
and installed by the execution owner while guest execution is stopped. The worker
loop should collect ready results without waiting for unfinished compilation;
snapshot validation and dispatch publication belong to the JIT integration.
