# Worker tasks and messages

Enable the machine's `worker` feature for `worker::Thread`. This is a direct
reexport of `web-thread-select`; the Wasm runtime exposes the same worker API
through its own `worker` feature. Use `ExecutionWorker` for an emulation session
with cooperative CPU quanta and explicit stop; see [execution scheduling](worker-execution.md).
Use `Thread` directly for other tasks, keeping it alive until accepted work finishes.

Pass the shared allocation as the task context. Capture configuration and Rust
channel endpoints in the closure, then construct the CPU inside the worker:

```rust,ignore
use relocade_machine::{worker::Thread, Cpu, Memory};

let memory = Memory::new(machine_config.clone())?;
memory.view().write(0x100, &program)?;
let worker = Thread::new();
let task = worker.run_send(memory.backing().clone(), move |backing| async move {
    let memory = Memory::with_backing(machine_config, backing)?;
    let mut cpu = Cpu::new(memory, cpu_config, Default::default(), ()).await?;
    Ok::<_, relocade_machine::Error>(cpu.run(budget).await)
});

// The caller can retain mapped RAM access while the worker executes.
let view = memory.view();
let exit = task.await??;
```

`SharedMemory` and compiled `Module` handles support the library's `Post`
transport directly. Native handles move as Rust values. Browser transport
preserves their JavaScript objects; shared-memory bytes are shared, not copied.
The receiving worker prepares its own local access to the allocation. Retained
handles remain usable after the allocating worker exits.

Machine configuration, command payloads and channel endpoints remain ordinary
Rust data. They use `Send` closure captures and Rust channels without
serialization. Once running on its worker, a future can retain non-Send values
such as `Rc`, the CPU and browser engine objects. Construct those values inside
the task. `Thread::run_send` returns Send results through Rust memory;
`Thread::run` posts its result and can return a shared allocation or module.

Host services are ordinary async functions that borrow `CpuContext` and can await
Rust channel input. `Cpu::run` drives execution; dropping a run waiter preserves
the pending service for a later run. The
[CPU worker example](../../../tests/browser/src/cpus.rs) exercises this complete
flow on native and browser workers: it captures configuration and channels,
constructs local state, waits for input while the caller accesses mapped memory,
resumes guest execution, and reuses the allocation on a later worker.

## Browser build and lifetime

The browser suite uses one threaded Rust configuration throughout: atomics,
bulk memory, mutable globals, rebuilt standard library, shared imported memory,
and wasm-bindgen's TLS exports. Shared Rust host memory is distinct from shared
guest memory. Pages must be cross-origin isolated with COOP/COEP headers.

The application resolves `web-thread:wasm-shim` to worker-safe bindings. A small
adapter passes the library's `module` argument as wasm-bindgen's `module_or_path`.
The shared build setup and browser checks live in
[tests/browser](../../../tests/browser/README.md). Backend tests exercise engine
operations; worker integration checks exercise channel wakeups, shared-memory
lifetimes, compiled-module handoff and CPU execution.

Await accepted work before dropping its thread. Browser bootstrap failure
reporting and thread allocation reclamation are tracked in
[#439](https://github.com/koolkdev/relocade/issues/439). Receiving posted messages
requires an event-loop task opportunity on the receiving browser thread.
