use futures_channel::oneshot;
use relocade_machine::{worker::Thread, Machine, RuntimeState};
use relocade_machine::{CpuConfig, DescriptorMemory, MachineConfig, PagedMemoryMap};
use relocade_wasm_runtime::{Backend, WasmBackend};
use relocade_x86::{Compiler, CpuState, Descriptor, DescriptorTables, Segment, SegmentProfile};

#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub async fn shared_descriptors() -> Result<(), String> {
    check().await.map_err(|error| error.to_string())
}

async fn check() -> Result<(), Box<dyn std::error::Error>> {
    let image = DescriptorMemory::new(DescriptorTables::default(), None)?;
    let cpu = CpuConfig::new(
        Compiler::new(SegmentProfile::Flat32).target(),
        CpuState::default(),
    )?
    .with_descriptor_memory(&image)?;
    let config =
        MachineConfig::new(0x1000, PagedMemoryMap::default())?.with_descriptor_memory(image)?;
    let mut backend = Backend::new()?;
    let space = Machine::new(&mut backend, config)?;
    // Install a live unaccessed descriptor that differs from the initial image.
    let descriptor = Descriptor::from_bytes([0xff, 0xff, 0, 0x70, 0, 0xf2, 0x40, 0]);
    space.memory().descriptors().write_at(0x20, descriptor)?;
    let thread = Thread::new();
    let (ready, started) = oneshot::channel();
    let (resume, mut resumed) = oneshot::channel();
    let task = space.on_worker(&thread, move |imported| async move {
        let mut backend = Backend::with_state(RuntimeState::new(())).unwrap();
        let memory = imported.bind_cpu(&mut backend, cpu).unwrap();
        let mut context = backend.context();
        memory.with_host_call(&mut context, |call| {
            let load = call
                .descriptors()
                .resolve_user_segment(Segment::Fs, 0x23)
                .unwrap();
            assert!(load.needs_completion());
            ready.send(()).ok()?;
            // Keep the resolved load alive while the other worker accesses shared storage.
            // Cancellation drops the sender, releasing us through the error path.
            loop {
                match resumed.try_recv() {
                    Ok(Some(())) => return Some(load.complete().base),
                    Ok(None) => std::hint::spin_loop(),
                    Err(_) => return None,
                }
            }
        })
    });
    started.await?;
    let unchanged = space.memory().descriptors().read_at(0x20)?;
    space.memory().descriptors().write_at(0x28, descriptor)?;
    space.memory().view().write_backing(0, &[1])?;
    resume.send(()).unwrap();
    let base = task.await?;
    assert_eq!(base, Some(0x7000));
    assert_eq!(unchanged, descriptor);
    assert_eq!(space.memory().descriptors().read_at(0x28)?, descriptor);
    assert_eq!(
        space.memory().descriptors().read_at(0x20)?.to_bytes()[5],
        0xf3
    );
    space
        .memory()
        .descriptors()
        .write_at(0x20, Descriptor::default())?;
    space.memory().view().map_pages(0x4000..=0x4fff, 0, true)?;
    Ok(())
}
