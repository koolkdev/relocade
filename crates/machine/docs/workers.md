# Worker tasks and messages

Enable the machine's `worker` feature for `worker::{Thread, channel, Message}`.
The Wasm runtime exposes the same API through its own `worker` feature.
`Thread`, its tasks and transport errors come from `web-thread-select`.
`Thread::new()` starts a worker. Keep it alive until its accepted tasks finish.

Worker futures, backends and state may contain local values such as `Rc`.
`Thread::run(context, code)` posts its context to the worker and posts the result
back. The closure itself is `Send`; create local state inside it or pass engine
objects in the context. `Thread::run_send` posts the same context but returns a
`Send` result through Rust memory. Ordinary Rust channels can still carry `Send`
commands captured by the closure.

Use typed worker channels for messages sent after a worker starts:

```rust,ignore
use relocade_machine::{Memory, worker::{channel, Thread}};

let worker = Thread::new();
let (sender, receiver) = channel::<Memory>()?;
let task = worker.run_send(receiver, |mut receiver| async move {
    let memory = receiver.recv().await?.expect("guest memory");
    Ok::<_, relocade_machine::Error>(memory.view().read_u16(0x200)?)
});
sender.send(memory.clone())?;
drop(sender);
let value = task.await??;
```

`channel::<T>()` returns one `Sender<T>` and one `Receiver<T>` for `T: Post`.
Messages are unbounded and ordered. `send` enqueues a value; it does not
acknowledge delivery. `recv().await` returns the next value, or `None` after
queued messages have been consumed and the sender has been dropped. Cancelling
a receive future keeps the inbox and queued messages available for the next
receive.

Endpoints are moved rather than cloned. They can be posted as a task context,
a task result or a message on another channel. Transfer a receiver before its
first `recv` call: receiving attaches its inbox to that worker, and subsequent
browser transfers are rejected. On the browser, sending cannot synchronously
detect a closed peer. A failed endpoint handoff does not guarantee an end-of-stream
notification; observe task failures and drop their unused peer endpoints.

On native targets, `Post` values move as `Send` Rust values. Browser transport
uses Serde, structured cloning and each object's transfer list. Shared guest
`Memory` and runtime `SharedMemory` implement `Post` directly. Posting a clone
shares the bytes and creates usable local access at the receiver. No allocation
service or root worker needs to remain alive. CPU construction and mapped
access are described in [shared guest memory](shared-memory.md).

`Message<T>` adapts serializable values that need no transfer list. Compiled
backend modules preserve their JavaScript objects through this transport:

```rust,ignore
worker.run_send(Message(module), |Message(module)| async move {
    // Link the received compiled module to this worker's backend.
});
```

Use an endpoint's own `Post` implementation when transferring it; wrapping it
in `Message` would omit its transfer list. Backend memory keys remain with their
owning backend. Browser memory objects remain local Rust values and cross
workers through posting rather than a `Send` promise.

## Browser build and lifetime

The browser suite uses one threaded Rust configuration throughout: atomics,
bulk memory, mutable globals, rebuilt standard library, shared imported memory,
and wasm-bindgen's TLS exports. Shared Rust host memory is distinct from shared
guest memory. Pages must be cross-origin isolated with COOP/COEP headers.

The application resolves `web-thread:wasm-shim` to worker-safe bindings. A small
adapter passes the library's `module` argument as wasm-bindgen's `module_or_path`.
The shared build setup and browser checks live in
[tests/browser](../../../tests/browser/README.md). They cover Rust channel
wakeups, posted endpoints, receive cancellation, shared-memory growth, CPUs
sharing mapped memory, memory retained after nested workers exit, and
compiled-module handoff.

Workers can create nested workers. Finish their tasks before dropping their
handles or the spawning worker. Task transport errors are returned by the task.
Forced termination and thread allocation reclamation remain tracked in
[#439](https://github.com/koolkdev/relocade/issues/439). Receiving posted objects
requires an event-loop task opportunity on the receiving browser thread.
