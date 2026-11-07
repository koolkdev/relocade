use futures_channel::oneshot;
use relocade_machine::{
    timer_service, Clock, CpuConfig, Device, DispatchEntry, ExecutionWorker, ExitHandle, IoBus,
    MachineConfig, Memory, TimerHandle,
};
use relocade_pic::Pic;
use relocade_pit::Pit;
use relocade_x86::{
    Compiler, CpuState, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap, Segments,
};
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

const INTERRUPTS: usize = 64;

struct Reports {
    clock: TimerHandle,
    ready: Option<oneshot::Sender<()>>,
    host_busy: Arc<AtomicBool>,
    count: Arc<AtomicUsize>,
    samples: Vec<Duration>,
    exit: Option<ExitHandle>,
}

impl Device for Reports {
    fn write_port(&mut self, port: u16, _: u8, _: u32) {
        if port == 0x81 {
            self.ready.take().unwrap().send(()).unwrap();
            // Do not deliver the measured interrupts before the host enters its
            // synchronous wait; a delayed host callback must not bypass this test.
            let deadline = self.clock.now() + Duration::from_secs(2);
            while !self.host_busy.load(Ordering::Acquire) && self.clock.now() < deadline {
                std::hint::spin_loop();
            }
            assert!(self.host_busy.load(Ordering::Acquire));
        } else {
            self.samples.push(self.clock.now());
            self.count.store(self.samples.len(), Ordering::Release);
            if self.samples.len() == INTERRUPTS {
                self.exit.as_ref().unwrap().request();
            }
        }
    }
}

/// Delivers 500 Hz PIT interrupts to a busy guest while its host cannot relay wakes.
async fn timer_interrupts() -> Result<(), String> {
    let (clock, timers) = timer_service();
    let warmup = clock.clone();
    let (start, started) = oneshot::channel();
    let (ready, running) = oneshot::channel();
    let count = Arc::new(AtomicUsize::new(0));
    let worker_count = count.clone();
    let host_busy = Arc::new(AtomicBool::new(false));
    let worker_host_busy = host_busy.clone();
    let worker = ExecutionWorker::spawn(clock, async move |runner| {
        let (pic, pic_connection) = Pic::initialize(runner).await;
        let (_pit, pit_connection) = Pit::initialize(runner, &pic).await;
        let reports = Rc::new(RefCell::new(Reports {
            clock: runner.clock().clone(),
            ready: Some(ready),
            host_busy: worker_host_busy,
            count: worker_count,
            samples: Vec::with_capacity(INTERRUPTS),
            exit: None,
        }));
        let connection = runner.connect(reports.clone(), [], [0x80..=0x81]).await;
        let memory = Memory::new(
            MachineConfig::new(
                0x1000,
                PhysicalMemoryMap::new([(0..=0xfff, PhysicalMapping::Ram { backing_offset: 0 })])
                    .unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        // Program mode 3, divisor 2386 (approximately 500 Hz); unmask IRQ0.
        // Announce readiness, enable interrupts, then stay in a compiled busy loop.
        let program = [
            0xb0, 0x36, 0xe6, 0x43, 0xb0, 0x52, 0xe6, 0x40, 0xb0, 0x09, 0xe6, 0x40, 0xb0, 0xfe,
            0xe6, 0x21, 0xe6, 0x81, 0xfb, 0x90, 0xeb, 0xfd,
        ];
        memory.view().write(0x100, &program).unwrap();
        memory.view().write(8 * 4, &[0, 2, 0, 0]).unwrap();
        // IRQ0: preserve AX, count/report entry, acknowledge the PIC, IRET.
        memory
            .view()
            .write(
                0x200,
                &[
                    0x50, 0xff, 0x06, 0, 3, 0xe6, 0x80, 0xb0, 0x20, 0xe6, 0x20, 0x58, 0xcf,
                ],
            )
            .unwrap();
        let compiler = Compiler::new(ExecutionProfile::Real16);
        let config = CpuConfig::new(
            compiler.target(),
            CpuState {
                eip: 0x100,
                segments: Segments::real_mode(),
                ..CpuState::default()
            },
        )
        .unwrap();
        let mut cpu = runner
            .create_cpu(
                memory.clone(),
                config,
                IoBus::new([pic_connection, pit_connection, connection]),
            )
            .await
            .unwrap();
        cpu.context().cpu.set_sp(0xf00);
        let busy = compiler.compile_block(0x113, &program[19..], 32).unwrap();
        cpu.install_module(
            &busy.bytes,
            &[DispatchEntry {
                address: 0x113,
                export: &busy.entry,
            }],
        )
        .await
        .unwrap();
        reports.borrow_mut().exit = Some(cpu.exit_handle());
        started.await.unwrap();
        runner.run_cpu(&mut cpu).await;
        assert_eq!(memory.view().read_u16(0x300).unwrap(), INTERRUPTS as u16);
        let samples = reports.borrow().samples.clone();
        samples
    });
    let timers = timers.spawn_with_peer(worker.wake_peer());
    // Finish timer-worker startup and resource binding before blocking the host.
    warmup.sleep(Duration::ZERO).await;
    start.send(()).unwrap();
    running.await.unwrap();
    host_busy.store(true, Ordering::Release);
    let deadline = warmup.now() + Duration::from_secs(2);
    while count.load(Ordering::Acquire) < INTERRUPTS && warmup.now() < deadline {
        std::hint::spin_loop();
    }
    let delivered_without_host = count.load(Ordering::Acquire);
    let result = if delivered_without_host == INTERRUPTS {
        worker.join().await
    } else {
        worker.stop().await
    };
    timers.stop().await.map_err(|error| error.to_string())?;
    assert_eq!(
        delivered_without_host, INTERRUPTS,
        "timer delivery depended on host polling"
    );
    let samples = result.map_err(|error| error.to_string())?.unwrap();
    assert_eq!(samples.len(), INTERRUPTS);
    assert!(samples.windows(2).all(|pair| pair[0] <= pair[1]));
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn timer_interrupts_reach_a_busy_guest_without_host_polling() {
    pollster::block_on(timer_interrupts()).unwrap();
}

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn timer_interrupts_reach_a_busy_guest_without_host_polling() {
    timer_interrupts().await.unwrap();
}
