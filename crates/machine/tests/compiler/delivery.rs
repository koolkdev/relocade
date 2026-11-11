use super::*;
use futures_util::future::{select, Either};
use relocade_machine::Device;
use std::{cell::RefCell, rc::Rc, time::Duration};

#[derive(Default)]
struct BusyDevice {
    turns: u64,
}

impl Device for BusyDevice {
    async fn run(device: Rc<RefCell<Self>>) {
        loop {
            device.borrow_mut().turns += 1;
            relocade_machine::yield_now().await;
        }
    }
}

async fn receive_while_running(compiler: &relocade_machine::CompilerService) {
    let (mut machine, config) = setup();
    let memory = machine.memory();
    memory.view().write(0x100, &[0x40, 0xeb, 0xfd]).unwrap(); // INC AX; JMP 0100
    memory.view().write(0x200, &[0xbb, 42, 0, 0xeb, 0]).unwrap(); // MOV BX,42; JMP 0205
    let cpu = machine.create_cpu(config, Some(compiler)).unwrap();
    let execution = cpu
        .run_until(
            futures_timer::Delay::new(Duration::from_secs(20)),
            async |mut runner| {
                let target = runner.cpu().cpu_state().execution_target();
                let mut compiler = runner.take_compiler().unwrap();
                let exit = runner.cpu().exit_handle();
                runner
                    .cpu()
                    .install([
                        Hook::host(0x200, async || {
                            panic!("compiled entry must replace this hook")
                        }),
                        Hook::host(0x205, async move || exit.request()),
                    ])
                    .await
                    .unwrap();
                let busy = Rc::new(RefCell::new(BusyDevice::default()));
                runner.connect(busy.clone(), |_| {}).await;

                let state = runner.cpu().cpu_state();
                let code = {
                    let receive = pin!(async {
                        while state.instruction_count() == 0 {
                            relocade_machine::yield_now().await;
                        }
                        compiler
                            .request(
                                target,
                                vec![BlockRequest {
                                    address: 0x200,
                                    start_eip: 0x200,
                                    ..request()
                                }],
                            )
                            .unwrap();
                        compiler.recv().await.expect("compiler running")
                    });
                    let running = pin!(runner.run_cpu());
                    match select(receive, running).await {
                        Either::Left((code, _)) => code.unwrap(),
                        Either::Right((exit, _)) => {
                            panic!("guest loop exited before receipt: {exit:?}")
                        }
                    }
                };
                assert!(
                    runner.cpu().cpu_state().instruction_count() > 0,
                    "the interpreter continues during compilation and receipt"
                );
                assert!(busy.borrow().turns > 1, "device work remains runnable too");
                code.install(runner.cpu()).unwrap();
                runner.cpu().cpu_state().set_eip(0x200);
                assert_eq!(
                    runner.run_cpu().await,
                    ExecutionExit::Host(HostExit::Return)
                );
                assert_eq!(runner.cpu().cpu_state().bx(), 42);
            },
        )
        .await;
    let shutdown = machine.shutdown().await;
    assert_eq!(
        execution.unwrap(),
        Some(()),
        "receipt must finish while guest and device work stay busy"
    );
    shutdown.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn compiled_code_arrives_while_guest_and_device_work_remain_runnable() {
    pollster::block_on(compilation::run(receive_while_running));
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn compiled_code_arrives_while_guest_and_device_work_remain_runnable() {
    compilation::run(receive_while_running).await;
}
