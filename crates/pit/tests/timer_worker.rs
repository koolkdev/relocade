use futures_channel::oneshot;
use relocade_clock_test_support::ManualClock;
use relocade_machine::{
    timer_service, Clock, CpuConfig, CpuHandle, Device, DispatchEntry, ExecutionWorker, ExitHandle,
    Machine, MachineConfig, SessionConfig, Timestamp,
};
use relocade_pic::Pic;
use relocade_pit::{Pit, CHANNEL_0_PORT, CONTROL_PORT};
use relocade_x86::{
    Compiler, CompilerConfig, CpuState, EntryAssumptions, MemoryBackend, PhysicalMapping,
    PhysicalMemoryMap,
};
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    task::{Context, Poll, Waker},
    time::Duration,
};

const INTERRUPTS: usize = 64;

struct Reports {
    clock: Clock,
    ready: Option<oneshot::Sender<()>>,
    host_busy: Arc<AtomicBool>,
    count: Arc<AtomicUsize>,
    samples: Vec<Timestamp>,
    exit: Option<ExitHandle>,
}

impl Device for Reports {}

impl Reports {
    fn ready(&mut self) {
        self.ready.take().unwrap().send(()).unwrap();
        // Do not deliver the measured interrupts before the host enters its
        // synchronous wait; a delayed host callback must not bypass this test.
        let deadline = self
            .clock
            .deadline_after(self.clock.now(), Duration::from_secs(2));
        while !self.host_busy.load(Ordering::Acquire) && self.clock.now() < deadline {
            std::hint::spin_loop();
        }
        assert!(self.host_busy.load(Ordering::Acquire));
    }

    fn record(&mut self) {
        self.samples.push(self.clock.now());
        self.count.store(self.samples.len(), Ordering::Release);
        if self.samples.len() == INTERRUPTS {
            self.exit.as_ref().unwrap().request();
        }
    }
}

fn machine_and_cpu() -> (Machine, CpuHandle) {
    let mut machine = Machine::new(
        MachineConfig::new(
            0x1000,
            PhysicalMemoryMap::new([(0..=0xfff, PhysicalMapping::Ram { backing_offset: 0 })])
                .unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let cpu_config = CpuConfig::new(
        CompilerConfig::new(EntryAssumptions::REAL_MODE, MemoryBackend::PhysicalBus),
        CpuState {
            eip: 0x100,
            ..CpuState::real_mode()
        },
    )
    .unwrap();
    let prepared = machine.create_cpu(cpu_config, None).unwrap();
    (machine, prepared)
}

/// Delivers 500 Hz PIT interrupts to a busy guest while its host cannot relay wakes.
async fn timer_interrupts() -> Result<(), String> {
    let (clock, timers) = timer_service();
    let warmup = clock.clock();
    let (start, started) = oneshot::channel();
    let (ready, running) = oneshot::channel();
    let count = Arc::new(AtomicUsize::new(0));
    let worker_count = count.clone();
    let host_busy = Arc::new(AtomicBool::new(false));
    let worker_host_busy = host_busy.clone();
    let (machine, prepared) = machine_and_cpu();
    let worker = ExecutionWorker::spawn(
        prepared,
        SessionConfig::host().with_host_clock(clock),
        async move |mut runner| {
            let pic = Pic::initialize(&runner);
            let _pit = Pit::initialize(&runner, &pic).await;
            let reports = Rc::new(RefCell::new(Reports {
                clock: runner.clock().clone(),
                ready: Some(ready),
                host_busy: worker_host_busy,
                count: worker_count,
                samples: Vec::with_capacity(INTERRUPTS),
                exit: None,
            }));
            runner
                .connect(reports.clone(), |io| {
                    let callbacks = relocade_machine::DeviceCallbacks::new(&reports);
                    let read = relocade_machine::PortRead::new(|_, _, _| u32::MAX);
                    io.bind(
                        0x80,
                        &read,
                        &relocade_machine::PortWrite::low_byte(
                            callbacks.write(|_, reports, _| reports.record()),
                        ),
                    );
                    io.bind(
                        0x81,
                        &read,
                        &relocade_machine::PortWrite::low_byte(
                            callbacks.write(|_, reports, _| reports.ready()),
                        ),
                    );
                })
                .await;
            let memory = runner.memory();
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
            let compiler = Compiler::from_config(CompilerConfig::new(
                EntryAssumptions::REAL_MODE,
                MemoryBackend::PhysicalBus,
            ));

            let cpu = runner.cpu();
            cpu.context().cpu.set_sp(0xf00);
            let busy = compiler.compile_block(0x113, &program[19..], 32).unwrap();
            cpu.install_module(
                &busy.bytes,
                &[DispatchEntry {
                    key: 0x113,
                    export: &busy.entry,
                }],
            )
            .await
            .unwrap();
            reports.borrow_mut().exit = Some(cpu.exit_handle());
            started.await.unwrap();
            runner.run_cpu().await;
            assert_eq!(memory.view().read_u16(0x300).unwrap(), INTERRUPTS as u16);
            let samples = reports.borrow().samples.clone();
            samples
        },
    );
    let timers = timers.spawn_with_peer(worker.wake_peer());
    // Finish timer-worker startup and resource binding before blocking the host.
    warmup.sleep(Duration::ZERO).await;
    start.send(()).unwrap();
    running.await.unwrap();
    host_busy.store(true, Ordering::Release);
    let deadline = warmup.deadline_after(warmup.now(), Duration::from_secs(2));
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
    machine.shutdown().await.unwrap();
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

async fn reprogram_separate_clock() {
    let (machine, cpu) = machine_and_cpu();
    let worker = ExecutionWorker::spawn(
        cpu,
        SessionConfig::host().with_host_clock(ManualClock::new()),
        async |runner| {
            let clock = ManualClock::new();
            let pic = Pic::initialize(&runner);
            let pit = Rc::new(RefCell::new(Pit::with_clock(clock.clock(), &pic)));
            runner
                .connect(pit.clone(), |io| Pit::configure_io(io, &pit))
                .await;
            pic.unmask_irq(relocade_pic::TIMER_IRQ);
            let io = runner.io();
            assert_eq!(
                clock.next_deadline(),
                Some(clock.at(relocade_pit::time_at(32_768)))
            );

            io.write_port(CONTROL_PORT, 1, 0x34);
            io.write_port(CHANNEL_0_PORT, 1, 4);
            io.write_port(CHANNEL_0_PORT, 1, 0);
            clock.advance_to(clock.at(Duration::from_nanos(3_353))); // Load and OUT low.
            relocade_machine::yield_now().await;
            assert!(io
                .poll_interrupt(&mut Context::from_waker(Waker::noop()))
                .is_pending());

            clock.advance_to(clock.at(Duration::from_nanos(4_191))); // First rising edge.
            assert_eq!(std::future::poll_fn(|cx| io.poll_interrupt(cx)).await, 8);
            relocade_pic::end_of_interrupt(&io, relocade_pic::TIMER_IRQ);

            // A control write at the next rising edge accounts for elapsed ticks
            // before it stops the counter. The PIC retains that final request.
            clock.advance_to(clock.at(relocade_pit::time_at(9)));
            io.write_port(CONTROL_PORT, 1, 0x30);
            let mut cx = Context::from_waker(Waker::noop());
            assert_eq!(io.poll_interrupt(&mut cx), Poll::Ready(8));
            relocade_pic::end_of_interrupt(&io, relocade_pic::TIMER_IRQ);
            clock.advance_by(Duration::from_secs(1));
            relocade_machine::yield_now().await;
            assert!(io.poll_interrupt(&mut cx).is_pending());
            io.write_port(CONTROL_PORT, 1, 0x36); // Control forces OUT high.
            assert_eq!(io.poll_interrupt(&mut cx), Poll::Ready(8));
        },
    );
    assert_eq!(worker.join().await.unwrap(), Some(()));
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn reprogrammed_channel_zero_delivers_irq0_on_its_own_clock() {
    pollster::block_on(reprogram_separate_clock());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn reprogrammed_channel_zero_delivers_irq0_on_its_own_clock() {
    reprogram_separate_clock().await;
}
