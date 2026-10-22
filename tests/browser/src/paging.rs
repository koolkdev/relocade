use futures_channel::oneshot;
use relocade_machine::{worker::Thread, Machine};
use relocade_machine::{MachineConfig, PagedMemoryMap};
use relocade_wasm_runtime::Backend;

fn configuration() -> MachineConfig {
    let mut map = PagedMemoryMap::default();
    map.map(0x4000..=0x4fff, 0, true).unwrap();
    MachineConfig::new(0x3000, map).unwrap()
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub async fn shared_page_tables() -> Result<(), String> {
    check().await.map_err(|error| error.to_string())
}

async fn check() -> Result<(), Box<dyn std::error::Error>> {
    let mut backend = Backend::new()?;
    let space = Machine::new(&mut backend, configuration())?;
    space.memory().view().write_backing(0, &[7])?;
    space.memory().view().write_backing(0x2000, &[42])?;
    space
        .memory()
        .view()
        .map_pages(0x4000..=0x4fff, 0x2000, true)?;
    let thread = Thread::new();
    let (ready, started) = oneshot::channel();
    let (resume, resumed) = oneshot::channel();
    let (released, stopped_reading) = oneshot::channel();
    let (remapped, changed) = oneshot::channel();
    let task = space.on_worker(&thread, move |imported| async move {
        let mut memory = imported.memory();
        let reader = memory.view();
        // The live table differs from the initial configuration sent to the worker.
        let initial = reader.read_u8(0x4000).unwrap();
        ready.send(()).unwrap();
        resumed.await.unwrap();
        let updated = reader.read_u8(0x4000).unwrap();
        released.send(()).unwrap();
        changed.await.unwrap();
        (initial, updated, reader.read_u8(0x4000).unwrap())
    });

    started.await?;
    let unchanged = space.memory().view().read_u8(0x4000)?;
    space.memory().view().write_u8(0x4000, 43)?;
    resume.send(()).unwrap();
    stopped_reading.await?;
    // The worker is waiting, so no transfer overlaps this mapping change.
    space.memory().view().map_pages(0x4000..=0x4fff, 0, true)?;
    remapped.send(()).unwrap();
    let observations = task.await?;
    assert_eq!(unchanged, 42);
    assert_eq!(observations, (42, 43, 7));
    Ok(())
}
