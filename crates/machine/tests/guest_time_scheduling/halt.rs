use super::{host::*, *};
use futures_util::StreamExt;
use relocade_machine::IoContext;
use std::task::Context;

#[path = "../support/interrupts.rs"]
mod interrupts;

struct TimerIrq {
    clock: Clock,
    deadline: Timestamp,
    observed: Option<Timestamp>,
    irq: interrupts::Irq,
}

impl Device for TimerIrq {
    async fn run(device: Rc<RefCell<Self>>) {
        let (clock, deadline) = {
            let device = device.borrow();
            (device.clock.clone(), device.deadline)
        };
        clock.sleep_until(deadline).await;
        let mut device = device.borrow_mut();
        device.observed = Some(clock.now());
        device.irq.raise();
    }

    fn poll_interrupt(&mut self, context: IoContext<'_>, cx: &mut Context<'_>) -> Poll<u8> {
        self.irq.poll_interrupt(context, cx)
    }
}

async fn halt_with_cpu_limit() {
    let (host, mut armed, source) = host();
    let setup = configuration();
    let mut machine = relocade_machine::Machine::new(setup.machine).unwrap();
    let prepared = machine.create_cpu(setup.cpu, None).unwrap();
    let worker = ExecutionWorker::spawn(
        prepared,
        SessionConfig::guest(config()).with_host_clock(source),
        async move |mut runner| {
            let clock = runner.clock().clone();
            let start = clock.now();
            let deadline = clock.deadline_after(start, Duration::from_millis(10));
            let device = Rc::new(RefCell::new(TimerIrq {
                clock: runner.clock().clone(),
                deadline,
                observed: None,
                irq: interrupts::Irq::default(),
            }));
            prepare_cpu(&mut runner, &[0xfb, 0xf4]).await; // STI; HLT.
            let state = runner.cpu().cpu_state();
            state.set_sp(0x8000);
            runner
                .cpu()
                .memory()
                .view()
                .write(0x180, &[0, 3, 0, 0])
                .unwrap();
            runner.cpu().memory().view().write(0x300, &[0xcf]).unwrap(); // IRET.
            let exit = runner.cpu().exit_handle();
            let cpu = runner.cpu();
            cpu.install([Hook::host(0x102, async move || exit.request())])
                .await
                .unwrap();
            runner.connect(device.clone(), |_| {}).await;
            assert_eq!(
                runner.run_cpu().await,
                ExecutionExit::Host(HostExit::Return)
            );
            assert_eq!(device.borrow().observed, Some(deadline));
            assert_eq!(device.borrow().irq.acknowledged, 1);
            assert_eq!(
                runner.cpu().memory().view().read_u16(0x7ffa).unwrap(),
                0x102
            );
            assert_eq!(state.instruction_count(), 3); // STI; HLT; IRET.
            assert_eq!(
                runner.clock().now(),
                clock.deadline_after(start, Duration::from_millis(11))
            );
        },
    );
    // STI and HLT consumed two guest ticks. While the CPU is halted, the
    // remaining eight ticks elapse at the device clock's nominal 1 kHz.
    let device_timer = host.at(Duration::from_millis(8));
    assert_eq!(armed.next().await, Some(device_timer));
    host.advance_to(device_timer);
    assert_eq!(worker.join().await.unwrap(), Some(()));
    assert_eq!(armed.next().await, None);
    assert_eq!(host.next_deadline(), None);
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn halted_cpu_waits_directly_for_its_next_device_irq() {
    pollster::block_on(halt_with_cpu_limit());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn halted_cpu_waits_directly_for_its_next_device_irq() {
    halt_with_cpu_limit().await;
}

async fn halted_at_instruction_limit() {
    use futures_lite::future::poll_once;
    use relocade_clock_test_support::ManualClock;
    use relocade_machine::{worker::wake_connection, GuestTimeConfig};

    let setup = configuration();
    let mut machine = relocade_machine::Machine::new(setup.machine).unwrap();
    let cpu = machine.create_cpu(setup.cpu, None).unwrap();
    let (_host, endpoint) = wake_connection(cpu.interrupt_handle());
    let host = ManualClock::new();
    let timing = GuestTimeConfig::new(NonZeroU64::new(1_000).unwrap())
        .with_cpu_limit(NonZeroU64::new(250).unwrap());
    let runner = Runner::with_guest_time(timing, host.clock(), cpu, endpoint)
        .await
        .unwrap();
    runner.memory().view().write(0x100, &[0xf4]).unwrap(); // HLT.
    let mut session = pin!(
        runner.run(std::future::pending::<()>(), async |mut runner| {
            let state = runner.cpu().cpu_state();
            let exit = runner.cpu().exit_handle();
            let clock = runner.clock().clone();
            runner
                .start_task(async move {
                    clock.sleep(Duration::from_millis(2)).await;
                    assert_eq!(state.instruction_count(), 1);
                    exit.request();
                })
                .await;
            runner.run_cpu().await
        })
    );

    // HLT retires the entire one-instruction allowance. The device has one
    // millisecond left on its guest timer. The CPU's next credit is at 4 ms.
    assert!(poll_once(session.as_mut()).await.is_none());
    host.advance_by(Duration::from_millis(1));
    assert_eq!(
        poll_once(session.as_mut()).await,
        Some(Some(ExecutionExit::Host(HostExit::Return))),
    );
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_halted_cpu_at_its_limit_allows_an_earlier_device_deadline() {
    pollster::block_on(halted_at_instruction_limit());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn a_halted_cpu_at_its_limit_allows_an_earlier_device_deadline() {
    relocade_machine::worker::Thread::new()
        .run(halted_at_instruction_limit)
        .await
        .unwrap();
}
