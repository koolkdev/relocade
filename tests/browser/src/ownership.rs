use futures_channel::oneshot;
use relocade_machine::{worker::Thread, Hook, Machine, RuntimeState};
use relocade_machine::{CpuConfig, MachineConfig, MemoryMap, PagedMemoryMap};
use relocade_wasm_runtime::Backend;
use relocade_x86::{
    Compiler, CpuState, ExecutionExit, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap,
    SegmentProfile, Segments,
};
use std::{
    ops::ControlFlow,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

fn configuration(profile: ExecutionProfile) -> MachineConfig {
    let map = match profile {
        ExecutionProfile::Real16 => {
            PhysicalMemoryMap::new([(0..=0x1fff, PhysicalMapping::Ram { backing_offset: 0 })])
                .unwrap()
                .into()
        }
        ExecutionProfile::Protected(_) => {
            let mut map = PagedMemoryMap::default();
            map.map(0..=0x1fff, 0, true).unwrap();
            MemoryMap::Paged(map)
        }
    };
    MachineConfig::new(0x2000, map).unwrap()
}

fn cpu(profile: ExecutionProfile) -> CpuConfig {
    CpuConfig::new(
        Compiler::new(profile).target(),
        CpuState {
            eip: 0x100,
            segments: match profile {
                ExecutionProfile::Real16 => Segments::real_mode(),
                ExecutionProfile::Protected(_) => Segments::flat32(),
            },
            ..CpuState::default()
        },
    )
    .unwrap()
}

fn initializing_module() -> Vec<u8> {
    use wasm_encoder::{ConstExpr, DataSection, ImportSection, MemoryType, Module};
    let mut module = Module::new();
    let mut imports = ImportSection::new();
    imports.import(
        "relocade",
        "guest",
        MemoryType {
            minimum: 1,
            maximum: Some(65536),
            memory64: false,
            shared: true,
            page_size_log2: None,
        },
    );
    module.section(&imports);
    let mut data = DataSection::new();
    data.active(0, &ConstExpr::i32_const(0x200), [0x55]);
    module.section(&data);
    module.finish()
}

struct Release(Arc<AtomicBool>);

impl Drop for Release {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub async fn shared_address_space() -> Result<(), String> {
    for profile in [
        ExecutionProfile::Real16,
        ExecutionProfile::Protected(SegmentProfile::Flat32),
    ] {
        check(profile).await.map_err(|error| error.to_string())?;
    }
    Ok(())
}

async fn check(profile: ExecutionProfile) -> Result<(), Box<dyn std::error::Error>> {
    let mut backend = Backend::with_state(RuntimeState::new(()))?;
    let space = Machine::new(&mut backend, configuration(profile))?;
    space.memory().view().write(0x100, &[0xeb, 0xfe])?;
    let version = space.memory().snapshot(0, &mut [])?;
    let worker_version = version.clone();
    let thread = Thread::new();
    let (ready, started) = oneshot::channel();
    let resume = Arc::new(AtomicBool::new(false));
    let release = Release(Arc::clone(&resume));
    let task = space.on_worker(&thread, move |imported| async move {
        let backend = Backend::with_state(RuntimeState::new(Some(ready))).unwrap();
        let mut runtime = imported.create_cpu(backend, cpu(profile)).await.unwrap();
        runtime
            .install_module(&worker_version, &initializing_module(), &[])
            .await
            .unwrap();
        runtime
            .install_hooks([Hook::new(0x100, move |call| {
                call.memory().write_u16(0x200, 0x4321).unwrap();
                let ready: &mut Option<oneshot::Sender<()>> = call.state_mut();
                ready.take().unwrap().send(()).unwrap();
                // Keep this CPU inside its guest call while the other worker
                // executes and writes through the same backing.
                while !resume.load(Ordering::Acquire) {
                    std::hint::spin_loop();
                }
                ControlFlow::Break(ExecutionExit::HostReturn)
            })])
            .await
            .unwrap();
        assert_eq!(runtime.run(1).unwrap(), ExecutionExit::HostReturn);
        runtime.id()
    });

    started.await?;
    let worker_cpus = space.cpu_count();
    // Attachment only initializes private CPU storage and can overlap execution.
    let mut peer = space.create_cpu(backend, cpu(profile)).await?;
    let all_cpus = space.cpu_count();
    let initial = space.memory().view().read_u16(0x200)?;
    let initializer = initializing_module();
    peer.install_module(&version, &initializer, &[]).await?;
    let initialized = space.memory().view().read_u16(0x200)?;
    let exit = peer.run(1)?;
    let instruction_count = peer.context().cpu().instruction_count();
    peer.context().memory().write_u16(0x200, 0xabcd)?;
    peer.context().memory().write_backing(0x202, &[0xee])?;
    let written = space.memory().view().read_u16(0x200)?;
    let backing = space.memory().view().read_u8(0x202)?;
    drop(release);
    let worker_id = task.await?;
    assert_eq!(worker_cpus, 1);
    assert_eq!(all_cpus, 2);
    assert_eq!(space.cpu_count(), 1);
    assert_ne!(worker_id, peer.id());
    assert_eq!(initial, 0x4321);
    assert_eq!(initialized, 0x4355);
    assert_eq!(exit, ExecutionExit::BudgetExhausted);
    assert_eq!(instruction_count, 1);
    assert_eq!(written, 0xabcd);
    assert_eq!(backing, 0xee);
    Ok(())
}
