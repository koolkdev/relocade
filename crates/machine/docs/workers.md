# Worker tasks and messages

The machine exposes `worker::Worker`, `worker::Thread` and `ExecutionWorker` without
feature flags. `Worker<T>` owns one task, its result and cooperative shutdown.
`Thread` is a reusable execution thread whose caller retains individual task handles.
`CpuHandle::spawn` transfers an existing CPU to a worker and supplies its
`Runner`; CPU-level work can use `Cpu` directly.

`Worker::spawn` invokes a task with its cancellation receiver. `Worker::spawn_with`
also transfers one initial value through the existing typed channel. It observes
cancellation while waiting for the value and releases the startup inbox before
calling the task with that value and its cancellation receiver. A send failure
requests cancellation and waits for worker cleanup before being reported.

```rust,ignore
let worker = Worker::spawn_with(initial_data, run_task);
let result = worker.join().await?;
```

After startup, the task observes its cancellation receiver and finishes its own cleanup.
Completion returns `Some(result)` if the task ran, or `None` if startup was stopped
before its initial data arrived. `WorkerError` reports channel and task failures.
`request_stop()` signals cancellation without releasing the owner;
`stop().await` signals cancellation and joins;
`join().await` waits for normal completion. `finished(&mut self)` permits racing
completion against another event while retaining the owner. Dropping that wait
does not cancel the task; consume its completed result only once.
Timer drivers, `ExecutionWorker` and graphics use this shared lifecycle.

For CPUs owned by one machine, prepare state before choosing a worker:

The application supplies `compiler_jobs` from its [shared compilation pool](compilation.md)
and keeps that pool running through machine shutdown.

```rust,ignore
let mut machine = Machine::new(machine_config, compiler_jobs.clone())?;
let cpu = machine.create_cpu(cpu_config)?;
machine.cpus()[0].set_ax(42);
let worker = Worker::spawn_with(cpu, run_cpu);
let result = worker.join().await;
let registers = machine.cpus()[0].snapshot();
machine.shutdown().await?;
let result = result?;
```

`Machine` retains shared memory and CPU state. `CpuHandle` is a single-use execution
capability; its derived `Transfer` carries initialized storage, memory and
configuration together. The receiving task constructs its backend with
`cpu.into_cpu(io).await`, preserving register edits made before transfer.
Host state access requires a stopped boundary, such as before transfer or after
joining execution. A state handle alone cannot construct another backend.

For lower-level task submission, `Thread::run` invokes a factory on the thread,
while `Thread::listen` gives it a local `Receiver<T>`. Pass the memory handle
directly and construct the CPU inside a named task:

```rust,ignore
use relocade_machine::{worker::{Receiver, Thread}, Cpu, CpuConfig, Memory};
use relocade_x86::ExecutionExit;

type Error = Box<dyn std::error::Error + Send + Sync>;

async fn run_cpu(mut input: Receiver<Memory>, config: CpuConfig) -> Result<ExecutionExit, Error> {
    let memory = input.recv().await.ok_or("memory sender closed")??;
    let mut cpu = Cpu::new(memory, config, Default::default()).await?;
    Ok(cpu.run().await)
}

let memory = Memory::new(machine_config)?;
memory.view().write(0x100, &program)?;
let thread = Thread::new();
let (mut input, task) = thread.listen(move |input| run_cpu(input, cpu_config))?;
input.send(memory.clone())?;

// The caller can retain mapped RAM access while the worker executes.
let view = memory.view();
let exit = task.await??;
```

`Memory` derives `Transfer`. Each field's type defines its transfer behavior:
`SharedMemory` posts its engine handle, and `Arc<MachineConfig>` retains its Rust
allocation. Named, tuple and unit structs compose those definitions without field
annotations. Receiving a message reconstructs the complete value on that worker.

```rust,ignore
#[derive(worker::Transfer)]
struct CpuResources {
    storage: CpuStorage,
    memory: Memory,
}
```

The generated implementation carries typed Rust state and a platform payload,
then decodes their fields on the receiving worker. Callers do not extract backing
or supply reconstruction closures. Every field needs `Transfer`; being `Send`
alone does not select an encoding. A shared `Arc<T>` retains its allocation when
`T: Send + Sync`, without requiring transfer behavior on its referent.

`Thread::listen` accepts messages before startup finishes and throughout the task's
life. For another direction, create `worker::channel::<T>()` on the receiving
thread and transfer its `Sender<T>` to the producer. The receiver stays local;
the sender can itself be sent through another channel to establish a reverse
direction. Each channel has one sender, preserves order, and uses an unbounded
queue. `send` posts immediately and `recv().await` receives a complete value.

```rust,ignore
use relocade_machine::worker::{self, Receiver, Sender, Thread};

async fn compile_and_send(mut input: Receiver<Sender<CompiledUnit>>) -> Result<(), worker::ChannelError> {
    let mut sender = input.recv().await.unwrap()?;
    sender.send(compile_unit().await?)
}

let (sender, mut compiled) = worker::channel::<CompiledUnit>()?;
let thread = Thread::new();
let (mut input, task) = thread.listen(compile_and_send)?;
input.send(sender)?;
let unit = compiled.recv().await.transpose()?;
task.await??;
```

Dropping a receive wait preserves its message. Dropping the sender lets the
receiver drain accepted messages before returning `None`; dropping the receiver
closes further sends. Browser object delivery still requires an event-loop turn
on the receiver. `Receiver::with_executor` lets the worker executor arrange those
turns after Rust state confirms posting. `Executor::with_host_timer` supplies an
independent timer and a shared execution interval before each turn. Receivers
register pending payloads while the executor owns delivery timing and gates its
complete activity set. The channel retains message ownership. See
[background compilation](compilation.md) for the Runner integration.

Use `CpuStorage::new()` when the host needs an execution-interrupt handle before
CPU construction. Retain `storage.interrupt_handle()`, send the storage through
the worker's inbox, then call `storage.into_cpu(memory, cpu_config, io).await` on the
execution worker. The storage is consumed by one CPU. Its signal is initialized
before handles are published; configuring CPU registers and the model preserves
requests made during construction. Neither preparation nor construction starts
guest execution. `Cpu::new` uses the same storage path when no early handle is needed.

`SharedMemory` and compiled `Module` handles implement `Transfer` and can be
sent directly through typed channels. The type defines its Rust state,
engine payload and reconstruction. Native handles move as Rust values. Browser
transport preserves their JavaScript objects; shared-memory bytes are shared, not copied.
The receiving worker prepares its own local access to the allocation. Retained
handles remain usable after the allocating worker exits.

Machine configuration, command payloads and channel endpoints remain ordinary
Rust data. They use `Send` closure captures and Rust channels without
serialization. Once running on its worker, a future can retain non-Send values
such as `Rc`, the CPU and browser engine objects. Construct those values inside
the task. `Thread::run` and `Thread::listen` return Send results through Rust memory;
send resources back through a channel when the result contains an engine object.
`Pool` accepts these same `Thread` owners for reusable synchronous jobs.

Host services are ordinary async functions capturing CPU state, memory, I/O and
service resources. They await Rust channel input without retaining a backend
borrow. `Cpu::run` drives execution; dropping a run waiter preserves the pending
service for a later run. The
[CPU worker example](../../../tests/browser/src/cpus.rs) exercises this complete
flow on native and browser workers: it passes memory handles and captures channels,
constructs local state, waits for input while the caller accesses mapped memory,
resumes guest execution, and reuses the allocation on a later worker.

`wake_connection(target)` binds a fixed notification resource on the host and
returns a host guard plus Send endpoint metadata. Capture the endpoint alongside
an already posted resource, then call `endpoint.bind(local_target)` on execution.
Both target handles must address the same signal. This reuses the resource's
existing transfer instead of posting it again through a registration channel.
`CpuHandle` startup does this automatically with its prepared CPU; callers attach
local devices through `runner.attach_io` or `Dos::new`. `WakeConnection` retains the
binding; the separate `worker::Executor` owns polling, stop handling and idle parking.

## Browser build and lifetime

Browser runtime builds use one threaded Rust configuration throughout: atomics,
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
