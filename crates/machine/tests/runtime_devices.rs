use relocade_clock_test_support::ManualClock;
mod support;
use futures_channel::{mpsc, oneshot};
use futures_util::StreamExt;
use relocade_machine::{Clock, Device, Hook};
use relocade_x86::{
    Compiler, CpuState, ExecutionExit, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap,
    Segments,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

struct Ticker {
    clock: Clock,
    ticks: Rc<Cell<u32>>,
    report: mpsc::UnboundedSender<u32>,
    active: Arc<AtomicBool>,
}

struct Active(Arc<AtomicBool>);
impl Drop for Active {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

impl Device for Ticker {
    fn read_port(&mut self, _port: u16, _bytes: u8) -> u32 {
        self.ticks.get()
    }

    async fn run(device: Rc<RefCell<Self>>) {
        let (clock, ticks, report, active) = {
            let device = device.borrow();
            (
                device.clock.clone(),
                device.ticks.clone(),
                device.report.clone(),
                device.active.clone(),
            )
        };
        active.store(true, Ordering::SeqCst);
        let _active = Active(active);
        let mut deadline = clock.add(clock.now(), Duration::from_millis(2));
        loop {
            clock.sleep_until(deadline).await;
            deadline = clock.add(deadline, Duration::from_millis(2));
            ticks.set(ticks.get() + 1);
            report.unbounded_send(ticks.get()).unwrap();
        }
    }
}

fn configuration() -> support::Configuration {
    support::configuration(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            eip: 0x100,
            segments: Segments::real_mode(),
            ..CpuState::default()
        },
        0x1000,
        PhysicalMemoryMap::new([(0..=0xfff, PhysicalMapping::Ram { backing_offset: 0 })]).unwrap(),
    )
    .unwrap()
}

async fn check_devices() {
    let clock = ManualClock::default();
    let active = Arc::new(AtomicBool::new(false));
    let worker_active = active.clone();
    let (report, mut ticks) = mpsc::unbounded();
    let (entered, waiting) = oneshot::channel();
    let setup = configuration();
    let mut machine = relocade_machine::Machine::new(setup.machine).unwrap();
    let prepared = machine.create_cpu(setup.cpu, None).unwrap();
    let worker = prepared.spawn_with_clock(
        {
            let controller = clock.clone();
            move || controller.clock()
        },
        async move |mut runner| {
            let device = Rc::new(RefCell::new(Ticker {
                clock: runner.clock().clone(),
                ticks: Rc::new(Cell::new(0)),
                report,
                active: worker_active.clone(),
            }));
            let io = runner.cpu().io();
            io.connect([runner
                .connect(device, [], std::iter::once(0x80..=0x80))
                .await]);
            assert!(
                worker_active.load(Ordering::SeqCst),
                "connect starts activity before returning"
            );

            let memory = runner.memory();
            let service_clock = runner.clock().clone();
            let cpu = runner.cpu();
            // Guest work after the sleeping service, followed by a completion hook.
            memory
                .view()
                .write(0x200, &[0xff, 0x06, 0, 3, 0xeb, 0])
                .unwrap();
            let entered = RefCell::new(Some(entered));
            cpu.install([
                Hook::host(0x100, {
                    let cpu = cpu.cpu_state();
                    async move || {
                        let sleep = service_clock.sleep(Duration::from_millis(5));
                        entered.borrow_mut().take().unwrap().send(()).unwrap();
                        sleep.await;
                        cpu.set_eip(0x200);
                    }
                }),
                Hook::host(0x206, {
                    let exit = cpu.exit_handle();
                    async move || exit.request()
                }),
            ])
            .await
            .unwrap();
            assert_eq!(
                runner.run_cpu().await,
                ExecutionExit::Host(relocade_x86::HostExit::Return)
            );
            let result = memory.view().read_u16(0x300).unwrap();
            (result, io.read_port(0x80, 1))
        },
    );
    waiting.await.unwrap();
    assert!(active.load(Ordering::SeqCst));
    clock.advance_to(clock.at(Duration::from_millis(2)));
    assert_eq!(ticks.next().await, Some(1));
    clock.advance_to(clock.at(Duration::from_millis(5)));
    assert_eq!(worker.join().await.unwrap(), Some((1, 2)));
    assert!(
        !active.load(Ordering::SeqCst),
        "main completion drops device activities"
    );
    machine.shutdown().await.unwrap();

    let worker_active = active.clone();
    let (ready, started) = oneshot::channel();
    let (report, _ticks) = mpsc::unbounded();
    let setup = configuration();
    let mut machine = relocade_machine::Machine::new(setup.machine).unwrap();
    let prepared = machine.create_cpu(setup.cpu, None).unwrap();
    let worker = prepared.spawn_with_clock::<()>(
        move || clock.clock(),
        async move |runner| {
            let device = Rc::new(RefCell::new(Ticker {
                clock: runner.clock().clone(),
                ticks: Rc::new(Cell::new(0)),
                report,
                active: worker_active.clone(),
            }));
            let _connection = runner
                .connect(device, [], std::iter::once(0x80..=0x80))
                .await;
            ready.send(()).unwrap();
            std::future::pending().await
        },
    );
    started.await.unwrap();
    assert!(active.load(Ordering::SeqCst));
    assert_eq!(worker.stop().await.unwrap(), None);
    assert!(
        !active.load(Ordering::SeqCst),
        "cancellation drops device activities"
    );
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn cpu_and_registered_devices_sleep_independently_and_end_together() {
    pollster::block_on(check_devices());
}
#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn cpu_and_registered_devices_sleep_independently_and_end_together() {
    check_devices().await;
}
