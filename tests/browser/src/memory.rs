use futures_channel::oneshot;
use relocade_machine::worker::Thread;
use relocade_wasm_runtime::{
    Backend, SharedMemory, WasmBackend, WasmMemoryAccess, WasmSharedMemory,
};

#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub async fn shared_memory() -> Result<(), String> {
    let creator = Thread::new();
    let shared = creator
        .run((), |()| async {
            SharedMemory::new(1, 2).map_err(|error| error.to_string())
        })
        .await
        .map_err(|error| error.to_string())??;
    drop(creator);
    shared.write(0, &[42]);
    let mut backend = Backend::new().map_err(|error| error.to_string())?;
    let memory = backend.import_shared_memory(&shared);
    let offset = 65536;
    let thread = Thread::new();
    let (ready, started) = oneshot::channel();
    let (written, updated) = oneshot::channel();
    let task = thread.run_send(shared.clone(), move |shared| async move {
        let mut access = shared.view();
        let mut initial = [0];
        access.read(0, &mut initial);
        ready.send(()).unwrap();
        updated.await.unwrap();
        let mut bytes = [0; 4];
        access.read(offset, &mut bytes);
        let scalar = access.load::<u64>(offset + 1);
        access.store(offset + 9, 0xfedc_ba98_7654_3210_u64);
        (initial, bytes, scalar)
    });

    started.await.map_err(|error| error.to_string())?;
    // The receiving worker already holds its own handle. Growing and writing
    // here must remain visible through that handle, not a copied snapshot.
    backend
        .ensure_memory_size(&memory, 2 * 65536)
        .map_err(|error| error.to_string())?;
    backend
        .memory(&memory)
        .write(65536, &[1, 2, 3, 4, 5, 6, 7, 8, 0xf9]);
    written.send(()).unwrap();
    let (initial, bytes, scalar) = task.await.map_err(|error| error.to_string())?;
    drop(thread);
    assert_eq!(initial, [42]);
    assert_eq!(bytes, [1, 2, 3, 4]);
    assert_eq!(scalar, 0xf908_0706_0504_0302);
    let mut stored = [0; 8];
    drop(backend);
    shared.read(65545, &mut stored);
    assert_eq!(stored, [0x10, 0x32, 0x54, 0x76, 0x98, 0xba, 0xdc, 0xfe]);
    Ok(())
}
