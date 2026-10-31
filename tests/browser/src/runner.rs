use futures_channel::oneshot;
use futures_util::future::join;
use relocade_machine::{
    timer_service, Clock, Cpu, CpuConfig, Device, ExecutionWorker, Hook, HookResult, IoBus,
    MachineConfig, Memory, TimerHandle,
};
use relocade_x86::{
    Compiler, CpuState, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap, Segments,
};
use std::{
    cell::{Cell, RefCell},
    future::pending,
    rc::Rc,
    time::Duration,
};

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

struct ProgressDevice {
    clock: TimerHandle,
    memory: Memory,
    progressed: Option<oneshot::Sender<u16>>,
    dropped: Option<oneshot::Sender<()>>,
}

impl Device for ProgressDevice {
    async fn run(device: Rc<RefCell<Self>>) {
        let clock = device.borrow().clock.clone();
        // Registration precedes CPU initialization and guest compilation.
        while device.borrow().memory.view().read_u16(0x300).unwrap() == 0 {
            clock.sleep(Duration::from_millis(5)).await;
        }
        for _ in 0..2 {
            clock.sleep(Duration::from_millis(5)).await;
        }
        {
            let mut state = device.borrow_mut();
            let marker = state.memory.view().read_u16(0x300).unwrap();
            state.progressed.take().unwrap().send(marker).unwrap();
        }
        pending::<()>().await;
    }
}

impl Drop for ProgressDevice {
    fn drop(&mut self) {
        assert!(EXECUTION_OWNER.get());
        self.dropped.take().unwrap().send(()).unwrap();
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub async fn runner_workers() -> Result<(), String> {
    for pending_service in [false, true] {
        let (clock, timers) = timer_service();
        let (progressed, ticks) = oneshot::channel();
        let (dropped, mut destroyed) = oneshot::channel();
        let (device_dropped, mut device_destroyed) = oneshot::channel();
        let worker = ExecutionWorker::spawn(clock, async move |runner| {
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
            let io = IoBus::new([runner.connect(
                Rc::new(RefCell::new(ProgressDevice {
                    clock: runner.clock().clone(),
                    memory: memory.clone(),
                    progressed: Some(progressed),
                    dropped: Some(device_dropped),
                })),
                [],
                [],
            )]);
            let mut cpu = Cpu::new(
                memory.clone(),
                config,
                io,
                DropProbe {
                    local: Rc::new(()),
                    dropped: Some(dropped),
                },
            )
            .await
            .unwrap();
            if pending_service {
                cpu.install_hooks([Hook::<DropProbe>::new(0x100, async |call| {
                    let mut memory = call.memory.view();
                    memory.write_u16(0x300, 2).unwrap();
                    pending::<()>().await;
                    memory.write_u16(0x300, 3).unwrap();
                    HookResult::Continue
                })])
                .await
                .unwrap();
            }
            runner.run_cpu(&mut cpu).await;
            panic!("busy or suspended execution must stop only through cancellation");
        });
        let control = async {
            assert_eq!(ticks.await.unwrap(), if pending_service { 2 } else { 1 });
            assert_eq!(
                worker.stop().await.map_err(|error| error.to_string())?,
                None::<()>
            );
            // Completion includes destruction of the CPU's suspended service state.
            assert_eq!(destroyed.try_recv().unwrap(), Some(()));
            assert_eq!(device_destroyed.try_recv().unwrap(), Some(()));
            Ok::<(), String>(())
        };
        let ((), result) = join(timers.run(), control).await;
        result?;
    }
    let (clock, timers) = timer_service();
    let worker = ExecutionWorker::spawn(clock, async |_| 42);
    let ((), result) = join(timers.run(), worker.join()).await;
    assert_eq!(result.map_err(|error| error.to_string())?, Some(42));
    Ok(())
}
