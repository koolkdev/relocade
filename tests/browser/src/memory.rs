use futures_channel::oneshot;
use relocade_compiler::abi::Memory;
use relocade_machine::worker::{channel, Thread};
use relocade_wasm_runtime::{
    Backend, SharedMemory, WasmBackend, WasmMemoryAccess, WasmSharedMemory,
};

const SHARED: Memory = Memory {
    shared: true,
    maximum: Some(2),
    ..Memory::new("test", "shared", 1)
};

#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub async fn shared_memory() -> Result<(), String> {
    let mut backend = Backend::new().map_err(|error| error.to_string())?;
    let shared = SharedMemory::new(SHARED.minimum, SHARED.maximum.unwrap())
        .map_err(|error| error.to_string())?;
    let memory = backend.import_shared_memory(&shared);
    backend.memory(&memory).write(0, &[42]);
    let (outgoing, incoming) = channel::<SharedMemory>().map_err(|error| error.to_string())?;
    let thread = Thread::new();
    let (ready, started) = oneshot::channel();
    let (written, updated) = oneshot::channel();
    let task = thread.run_send(incoming, move |mut incoming| async move {
        let shared = incoming.recv().await.unwrap().unwrap();
        assert!(incoming.recv().await.unwrap().is_none());
        let mut access = shared.view();
        let mut initial = [0];
        access.read(0, &mut initial);
        ready.send(()).unwrap();
        updated.await.unwrap();
        let mut bytes = [0; 4];
        access.read(65536, &mut bytes);
        let scalar = access.load::<u64>(65537);
        access.store(65545, 0xfedc_ba98_7654_3210_u64);
        (initial, bytes, scalar)
    });
    outgoing
        .send(backend.shared_memory(&memory).unwrap())
        .unwrap();
    drop(outgoing);

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
    assert_eq!(initial, [42]);
    assert_eq!(bytes, [1, 2, 3, 4]);
    assert_eq!(scalar, 0xf908_0706_0504_0302);
    let mut stored = [0; 8];
    backend.memory(&memory).read(65545, &mut stored);
    assert_eq!(stored, [0x10, 0x32, 0x54, 0x76, 0x98, 0xba, 0xdc, 0xfe]);
    Ok(())
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub async fn nested_shared_memory() -> Result<(), String> {
    let parent = Thread::new();
    let shared = parent
        .run((), move |()| async move {
            let child = Thread::new();
            let shared = child
                .run((), move |()| async move {
                    let mut backend = Backend::new().unwrap();
                    let shared =
                        SharedMemory::new(SHARED.minimum, SHARED.maximum.unwrap()).unwrap();
                    let memory = backend.import_shared_memory(&shared);
                    backend.memory(&memory).store(7, 0x1234_u16);
                    backend.shared_memory(&memory).unwrap()
                })
                .await
                .unwrap();
            drop(child);
            assert_eq!(shared.view().load::<u16>(7), 0x1234);
            shared.view().store(7, 0x5678_u16);
            shared
        })
        .await
        .map_err(|error| error.to_string())?;
    drop(parent);
    assert_eq!(shared.view().load::<u16>(7), 0x5678);
    Ok(())
}
