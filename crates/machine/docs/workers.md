# Worker tasks and messages

Enable the runtime's `worker` feature for `runtime::worker::{Thread, Message}`.
`Thread` is the published `web-thread-select` library's task owner. The embedding
creates threads and keeps them alive until their tasks finish.

Rust commands and shared state use Send closure captures and ordinary Rust
channels. Worker-local futures, backends and state can contain non-Send values
such as `Rc`. `Thread::run_send` returns Send results through Rust memory.

`Message<T>` adapts a payload to the library's `Post` transport. Native payloads
move as Send Rust values. Browser payloads use Serde and structured cloning.
Message types can use ordinary derives without platform conditions:

```rust,ignore
use relocade_machine::worker::Message;
use relocade_wasm_runtime::WasmSharedMemory;

#[derive(serde::Serialize, serde::Deserialize)]
struct Reader<M> {
    memory: M,
    offset: u32,
}

let task = thread.run_send(
    Message(Reader { memory: shared_memory, offset: 65536 }),
    |Message(reader)| async move {
        let mut bytes = [0; 4];
        reader.memory.read(reader.offset, &mut bytes);
        bytes
    },
);
```

Browser backend `Module` and `SharedMemory` types preserve their JavaScript
objects when serialized. A received shared memory refers to the same backing
bytes; sending the handle neither copies those bytes nor synchronizes accesses.
Use the backend's independent shared-memory handle, not its store-local `Memory`
index. Objects requiring transfer lists are outside `Message`'s scope.

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
messages and compiled-module handoff.

Await accepted work before dropping its thread. Browser bootstrap failure
reporting and thread allocation reclamation are tracked in
[#439](https://github.com/koolkdev/relocade/issues/439). Receiving posted messages
requires an event-loop task opportunity on the receiving browser thread.
