use super::*;
use relocade_machine::{FarPointer, Runner};
use std::time::Duration;

async fn slice(runner: &mut Runner) {
    let clock = runner.clock().clone();
    let exit = runner.cpu().exit_handle();
    let (result, ()) = futures_util::future::join(runner.run_cpu(), async move {
        clock.sleep(Duration::from_millis(20)).await;
        exit.request();
    })
    .await;
    assert_eq!(result, ExecutionExit::Host(HostExit::Return));
}

async fn automatic(compiler: &relocade_machine::CompilerService) {
    // Discovery follows the untaken branch into unsupported bytes in the second
    // candidate. Execution remains valid and must continue through its fallback.
    for rejected in [false, true] {
        let (mut machine, config) = setup();
        machine
            .memory()
            .view()
            .write(0x100, &[0xe8, 0xfd, 0, 0xeb, 0xfb])
            .unwrap(); // CALL 0200; JMP 0100
        let callee: &[u8] = if rejected {
            &[0x40, 0x85, 0xff, 0x75, 0x1b, 0xc3] // INC AX; TEST DI,DI; JNZ 0220; RET
        } else {
            &[0x40, 0xc3] // INC AX; RET
        };
        machine.memory().view().write(0x200, callee).unwrap();
        machine.memory().view().write(0x220, &[0xf1]).unwrap();
        let cpu = machine.create_cpu(config, Some(compiler)).unwrap();
        let result = cpu
            .run_until(pending::<()>(), async move |mut runner| {
                runner.cpu().cpu_state().set_sp(0x800);
                let target = FarPointer {
                    segment: 0,
                    offset: 0x200,
                };
                let deadline = runner
                    .clock()
                    .add(runner.clock().now(), Duration::from_secs(5));
                let mut previous = 0;
                loop {
                    let retired = runner.cpu().cpu_state().instruction_count();
                    slice(&mut runner).await;
                    assert!(runner.cpu().cpu_state().instruction_count() > retired);
                    let count = runner.cpu().call_count(target);
                    if count >= 1024 && count == previous {
                        // Guest calls continue but this destination now resolves, for
                        // both a compiled function and a rejected-function fallback.
                        slice(&mut runner).await;
                        assert_eq!(runner.cpu().call_count(target), count);
                        break;
                    }
                    assert!(
                        runner.clock().now() < deadline,
                        "automatic installation did not complete"
                    );
                    previous = count;
                }
            })
            .await;
        machine.shutdown().await.unwrap();
        assert_eq!(result.unwrap(), Some(()));
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn hot_functions_install_automatically_and_rejections_stop_recording() {
    pollster::block_on(compilation::run(automatic));
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn hot_functions_install_automatically_and_rejections_stop_recording() {
    compilation::run(automatic).await;
}
