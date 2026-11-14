use super::*;
use relocade_machine::{FarPointer, FunctionRequest};
use relocade_x86::FunctionLimits;

async fn late_callee(compiler: &relocade_machine::CompilerService) {
    let (mut machine, config) = setup();
    let mut state = *config.cpu();
    state.registers.esp = 0x800;
    let config = CpuConfig::new(config.execution_target(), state).unwrap();
    machine
        .memory()
        .view()
        .write(
            0x100,
            &[
                0xe8, 0xfd, 0, // CALL 0200
                0xe8, 0xfa, 0, // CALL 0200 again
                0xe9, 0xf7, 0x0e, // JMP 1000, outside readable code
            ],
        )
        .unwrap();
    machine
        .memory()
        .view()
        .write(
            0x180,
            &[
                0xe8, 0x7d, 0, // CALL 0200 from another function
                0xe8, 0x7a, 0, // CALL 0200 again
                0xe9, 0x77, 0x0e, // JMP 1000, outside readable code
            ],
        )
        .unwrap();
    machine
        .memory()
        .view()
        .write(0x200, &[0x83, 0xc0, 7, 0xc3]) // ADD AX,7; RET
        .unwrap();
    let cpu = machine.create_cpu(config, Some(compiler)).unwrap();
    let result = cpu
        .run_until(pending::<()>(), async move |mut runner| {
            let target = runner.cpu().cpu_state().execution_target();
            let mut compiler = runner.take_compiler().unwrap();
            let request = |offset| FunctionRequest {
                entry: FarPointer { segment: 0, offset },
                limits: FunctionLimits::default(),
            };
            compiler
                .request_functions(target, vec![request(0x100), request(0x180)])
                .unwrap();
            let caller = compiler.recv().await.unwrap().unwrap();
            let exit = runner.cpu().exit_handle();
            runner
                .cpu()
                .install([Hook::host(0x1000, async move || exit.request())])
                .await
                .unwrap();
            caller.install(runner.cpu()).unwrap();
            for stage in 0..4 {
                if stage == 1 || stage == 3 {
                    if stage == 3 {
                        // Replace a callee at the same address while this CPU is stopped.
                        runner
                            .cpu()
                            .memory()
                            .view()
                            .write(0x200, &[0x83, 0xc0, 9, 0xc3]) // ADD AX,9; RET
                            .unwrap();
                    }
                    compiler
                        .request_functions(target, vec![request(0x200)])
                        .unwrap();
                    compiler
                        .recv()
                        .await
                        .unwrap()
                        .unwrap()
                        .install(runner.cpu())
                        .unwrap();
                }
                for entry in [0x100, 0x180] {
                    let state = runner.cpu().cpu_state();
                    state.set_eip(entry);
                    state.set_ax(0);
                    assert_eq!(
                        runner.run_cpu().await,
                        ExecutionExit::Host(HostExit::Return)
                    );
                    let state = runner.cpu().cpu_state();
                    assert_eq!(state.ax(), if stage == 3 { 18 } else { 14 });
                    assert_eq!(state.sp(), 0x800);
                }
            }
            // Installing a hook also updates existing callers.
            let exit = runner.cpu().exit_handle();
            runner
                .cpu()
                .install([Hook::host(0x200, async move || exit.request())])
                .await
                .unwrap();
            for entry in [0x100, 0x180] {
                let state = runner.cpu().cpu_state();
                state.set_eip(entry);
                state.set_sp(0x800);
                assert_eq!(
                    runner.run_cpu().await,
                    ExecutionExit::Host(HostExit::Return)
                );
                let state = runner.cpu().cpu_state();
                assert_eq!(state.eip(), 0x200);
                assert_eq!(state.sp(), 0x7fe);
            }
        })
        .await;
    machine.shutdown().await.unwrap();
    assert_eq!(result.unwrap(), Some(()));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn installed_caller_reaches_future_and_replacement_callee_modules() {
    pollster::block_on(compilation::run(late_callee));
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn installed_caller_reaches_future_and_replacement_callee_modules() {
    compilation::run(late_callee).await;
}
