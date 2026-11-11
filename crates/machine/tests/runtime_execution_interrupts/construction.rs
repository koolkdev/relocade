use super::*;
use futures_channel::{mpsc, oneshot};
use futures_util::StreamExt;
use relocade_machine::{timer_service, CpuStateAccess, Device};
use std::{cell::RefCell, rc::Rc};

struct IdleDevice {
    requests: Option<mpsc::UnboundedReceiver<oneshot::Sender<u64>>>,
    state: CpuStateAccess,
}

impl Device for IdleDevice {
    async fn run(device: Rc<RefCell<Self>>) {
        let mut requests = device.borrow_mut().requests.take().unwrap();
        while let Some(reply) = requests.next().await {
            reply
                .send(device.borrow().state.instruction_count())
                .unwrap();
        }
    }
}

async fn idle_devices() {
    let (clock, _timers) = timer_service();
    let setup = configuration();
    let mut machine = relocade_machine::Machine::new(setup.machine).unwrap();
    let prepared = machine.create_cpu(setup.cpu, None).unwrap();
    let worker = prepared.spawn(clock, async |mut runner| {
        // INC AX; JMP 0200, where the host service completes this guest run.
        prepare_cpu(&mut runner, &[0x40, 0xe9, 0xfc, 0], true).await;
        let state = runner.cpu().cpu_state();
        let exit = runner.cpu().exit_handle();
        runner
            .cpu()
            .install([relocade_machine::Hook::host(0x200, async move || {
                exit.request()
            })])
            .await
            .unwrap();
        let (send, requests) = mpsc::unbounded();
        let _device = runner
            .connect(
                Rc::new(RefCell::new(IdleDevice {
                    requests: Some(requests),
                    state: state.clone(),
                })),
                |_| {},
            )
            .await;
        let probe = async || {
            let (reply, received) = oneshot::channel();
            send.unbounded_send(reply).unwrap();
            received.await.unwrap()
        };
        assert_eq!(
            probe().await,
            0,
            "device work does not start prepared CPU execution"
        );
        assert_eq!(
            runner.run_cpu().await,
            ExecutionExit::Host(relocade_x86::HostExit::Return)
        );
        assert_eq!(
            probe().await,
            2,
            "devices remain active after guest completion"
        );
        assert_eq!(state.ax(), 1);
    });
    assert_eq!(worker.join().await.unwrap(), Some(()));
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn devices_run_before_and_after_guest_execution_without_starting_it() {
    pollster::block_on(idle_devices());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn devices_run_before_and_after_guest_execution_without_starting_it() {
    idle_devices().await;
}
