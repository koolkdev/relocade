use futures_channel::oneshot;
use relocade_compiler::abi::Memory;
use relocade_runtime::worker::{Message, Thread};
use relocade_wasm_runtime::{Backend, WasmBackend, WasmMemoryAccess, WasmSharedMemory};
use serde::{Deserialize, Serialize};

const SHARED: Memory = Memory {
    shared: true,
    maximum: Some(2),
    ..Memory::new("test", "shared", 1)
};

#[derive(Serialize, Deserialize)]
struct Reader<M> {
    memory: M,
    offset: u32,
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub async fn shared_memory() -> Result<(), String> {
    let mut backend = Backend::new().map_err(|error| error.to_string())?;
    let memory = backend
        .create_memory(SHARED)
        .map_err(|error| error.to_string())?;
    backend.write_memory(&memory, 0, &[42]);
    let reader = Reader {
        memory: backend.shared_memory(&memory).unwrap(),
        offset: 65536,
    };
    let thread = Thread::new();
    let (ready, started) = oneshot::channel();
    let (written, updated) = oneshot::channel();
    let task = thread.run_send(Message(reader), move |Message(reader)| async move {
        let mut initial = [0];
        reader.memory.read(0, &mut initial);
        ready.send(()).unwrap();
        updated.await.unwrap();
        let mut bytes = [0; 4];
        reader.memory.read(reader.offset, &mut bytes);
        (initial, bytes)
    });

    started.await.map_err(|error| error.to_string())?;
    // The receiving worker already holds its own handle. Growing and writing
    // here must remain visible through that handle, not a copied snapshot.
    backend
        .ensure_memory_size(&memory, 2 * 65536)
        .map_err(|error| error.to_string())?;
    backend.write_memory(&memory, 65536, &[1, 2, 3, 4]);
    written.send(()).unwrap();
    let (initial, bytes) = task.await.map_err(|error| error.to_string())?;
    assert_eq!(initial, [42]);
    assert_eq!(bytes, [1, 2, 3, 4]);
    Ok(())
}
