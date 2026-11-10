use super::{host::*, *};
use futures_util::StreamExt;
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

    fn poll_interrupt(&mut self, cx: &mut Context<'_>) -> Poll<u8> {
        self.irq.poll_interrupt(cx)
    }
}

async fn paced_halt() {
    let (host, mut armed, source) = host();
    let host_time = host.clone();
    let worker = ExecutionWorker::spawn_with_guest_time(
        move || config(source),
        async move |mut runner| {
            let clock = runner.clock().clone();
            let start = clock.now();
            let deadline = clock.add(start, Duration::from_millis(10));
            let device = Rc::new(RefCell::new(TimerIrq {
                clock: runner.clock().clone(),
                deadline,
                observed: None,
                irq: interrupts::Irq::default(),
            }));
            let io = IoBus::new([runner.connect(device.clone(), [], []).await]);
            prepare_cpu(&mut runner, &[0xfb, 0xf4], io).await; // STI; HLT.
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
                clock.add(start, Duration::from_millis(11))
            );
        },
    );
    // The halted CPU advances idle guest time through the existing paced slices.
    // No instructions retire while it waits for the timer IRQ at guest 10 ms.
    for millis in [4, 8] {
        let boundary = host_time.at(Duration::from_millis(millis));
        assert_eq!(armed.next().await, Some(boundary));
        host.advance_to(boundary);
    }
    assert_eq!(worker.join().await.unwrap(), Some(()));
    assert_eq!(host.next_deadline(), None);
    assert_eq!(armed.next().await, None);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn halted_cpu_advances_paced_guest_time_until_a_timer_irq() {
    pollster::block_on(paced_halt());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn halted_cpu_advances_paced_guest_time_until_a_timer_irq() {
    paced_halt().await;
}
