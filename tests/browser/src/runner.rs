use futures_channel::oneshot;
use futures_util::future::join;
use relocade_machine::{
    Clock, Cpu, CpuConfig, ExecutionWorker, Hook, HookResult, MachineConfig, Memory,
};
use relocade_x86::{
    Compiler, CpuState, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap, Segments,
};
use std::{cell::Cell, future::pending, rc::Rc, time::Duration};

thread_local! { static EXECUTION_OWNER: Cell<bool> = const { Cell::new(false) }; }

struct DropProbe {
    local: Rc<()>,
    dropped: Option<oneshot::Sender<()>>,
}
impl Drop for DropProbe {
    fn drop(&mut self) {
        assert!(EXECUTION_OWNER.get());
        assert_eq!(Rc::strong_count(&self.local), 1);
        self.dropped.take().unwrap().send(()).unwrap();
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub async fn runner_workers() -> Result<(), String> {
    for pending_service in [false, true] {
        let (progressed, ticks) = oneshot::channel();
        let (dropped, mut destroyed) = oneshot::channel();
        let worker = ExecutionWorker::spawn(move |runner| async move {
            EXECUTION_OWNER.set(true);
            let memory = Memory::new(
                MachineConfig::new(
                    0x1000,
                    PhysicalMemoryMap::new([(
                        0..=0xfff,
                        PhysicalMapping::Ram { backing_offset: 0 },
                    )])
                    .unwrap(),
                )
                .unwrap(),
            )
            .unwrap();
            // Write a marker, then consume CPU quanta indefinitely.
            memory
                .view()
                .write(0x100, &[0xc7, 0x06, 0, 3, 1, 0, 0xeb, 0xfe])
                .unwrap();
            let config = CpuConfig::new(
                Compiler::new(ExecutionProfile::Real16).target(),
                CpuState {
                    eip: 0x100,
                    segments: Segments::real_mode(),
                    ..CpuState::default()
                },
            )
            .unwrap();
            let mut cpu = Cpu::new(
                memory.clone(),
                config,
                Default::default(),
                DropProbe {
                    local: Rc::new(()),
                    dropped: Some(dropped),
                },
            )
            .await
            .unwrap();
            if pending_service {
                cpu.install_hooks([Hook::<DropProbe>::new(0x100, async |call| {
                    let memory = &mut call.memory;
                    memory.write_u16(0x300, 2).unwrap();
                    pending::<()>().await;
                    memory.write_u16(0x300, 3).unwrap();
                    HookResult::Continue
                })])
                .await
                .unwrap();
            }
            let timer = async {
                let clock = runner.clock();
                for _ in 0..2 {
                    clock
                        .wait_until(clock.now() + Duration::from_millis(5))
                        .await;
                }
                progressed
                    .send(memory.view().read_u16(0x300).unwrap())
                    .unwrap();
                pending::<()>().await;
            };
            join(runner.run(&mut cpu), timer).await;
            panic!("busy or suspended execution must stop only through cancellation");
        });
        assert_eq!(ticks.await.unwrap(), if pending_service { 2 } else { 1 });
        assert_eq!(
            worker.stop().await.map_err(|error| error.to_string())?,
            None::<()>
        );
        // Completion must include destruction of the CPU's suspended service state.
        assert_eq!(destroyed.try_recv().unwrap(), Some(()));
    }
    let worker = ExecutionWorker::spawn(|_| async { 42 });
    assert_eq!(
        worker.join().await.map_err(|error| error.to_string())?,
        Some(42)
    );
    Ok(())
}
