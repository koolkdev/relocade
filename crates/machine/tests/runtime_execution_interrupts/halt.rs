use super::{configuration, prepare_cpu};
use futures_channel::{mpsc, oneshot};
use futures_util::StreamExt;
use relocade_machine::{timer_service, Device, IoContext};
use std::{
    cell::RefCell,
    future::{poll_fn, Future},
    pin::pin,
    rc::Rc,
    task::{Context, Poll},
};

#[path = "../support/interrupts.rs"]
mod interrupts;

struct InputIrq {
    irq: interrupts::Irq,
    input: Option<oneshot::Receiver<()>>,
}

impl Device for InputIrq {
    async fn run(device: Rc<RefCell<Self>>) {
        let input = device.borrow_mut().input.take().unwrap();
        input.await.unwrap();
        device.borrow_mut().irq.raise();
    }

    fn poll_interrupt(&mut self, context: IoContext<'_>, cx: &mut Context<'_>) -> Poll<u8> {
        self.irq.poll_interrupt(context, cx)
    }
}

async fn halted_worker() {
    let (clock, _timers) = timer_service();
    let (send, input) = oneshot::channel();
    let (report, mut halted) = mpsc::unbounded();
    let (stopped, dropped) = oneshot::channel();
    let setup = configuration();
    let mut machine = relocade_machine::Machine::new(setup.machine).unwrap();
    let prepared = machine.create_cpu(setup.cpu, None).unwrap();
    let worker = prepared.spawn::<()>(clock, async move |mut runner| {
        runner
            .connect(
                Rc::new(RefCell::new(InputIrq {
                    irq: interrupts::Irq::default(),
                    input: Some(input),
                })),
                |_| {},
            )
            .await;
        prepare_cpu(&mut runner, &[0xfb, 0xf4, 0x40, 0xf4], true).await;
        let state = runner.cpu().cpu_state();
        state.set_sp(0x8000);
        runner
            .cpu()
            .memory()
            .view()
            .write(0x180, &[0, 3, 0, 0])
            .unwrap();
        runner.cpu().memory().view().write(0x300, &[0xcf]).unwrap();
        let _stopped = super::Stopped {
            state: state.clone(),
            report: Some(stopped),
        };
        let mut execution = pin!(runner.run_cpu());
        let mut last = 0;
        poll_fn(|cx| {
            let result = execution.as_mut().poll(cx);
            let ip = state.eip();
            if result.is_pending() && ip != last && matches!(ip, 0x102 | 0x104) {
                report
                    .unbounded_send((ip, state.instruction_count(), state.ax()))
                    .unwrap();
                last = ip;
            }
            result
        })
        .await;
        panic!("stopping a halted worker must drop its application");
    });
    assert_eq!(halted.next().await, Some((0x102, 2, 0)));
    // The registered device task still runs while CPU execution is pending in HLT.
    // Its ordinary input wake must reach the worker without a manual interrupt request.
    send.send(()).unwrap();
    assert_eq!(halted.next().await, Some((0x104, 5, 1)));
    assert_eq!(worker.stop().await.unwrap(), None);
    assert_eq!(dropped.await.unwrap(), 5);
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn halted_worker_drives_devices_and_accepts_cancellation() {
    pollster::block_on(halted_worker());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn halted_worker_drives_devices_and_accepts_cancellation() {
    halted_worker().await;
}
