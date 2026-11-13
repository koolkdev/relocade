use super::*;
use relocade_machine::{FarPointer, FunctionRequest};
use relocade_x86::{FunctionLimits, Segment, SegmentRegister};

async fn execute_function(compiler: &relocade_machine::CompilerService) {
    let (mut machine, config) = setup();
    let mut state = *config.cpu();
    state.segments.cs = SegmentRegister::real_mode(Segment::Cs, 0x10);
    state.eip = 0;
    state.registers.esp = 0x800;
    state.registers.ebx = 0x20;
    let config = CpuConfig::new(config.execution_target(), state).unwrap();
    machine
        .memory()
        .view()
        .write(
            0x100,
            &[
                0xb9, 2, 0, // MOV CX,2
                0x49, 0x75, 0xfd, // DEC CX; JNZ 0003
                0xe8, 7, 0, // CALL 0010
                0xb8, 42, 0, // MOV AX,42
                0xff, 0xe3, // JMP BX, leaving the discovered region
            ],
        )
        .unwrap();
    machine.memory().view().write(0x110, &[0x42, 0xc3]).unwrap(); // INC DX; RET
    let cpu = machine.create_cpu(config, Some(compiler)).unwrap();
    let result = cpu
        .run_until(pending::<()>(), async move |mut runner| {
            let target = runner.cpu().cpu_state().execution_target();
            let mut compiler = runner.take_compiler().unwrap();
            compiler
                .request_function(
                    target,
                    FunctionRequest {
                        entry: FarPointer {
                            segment: 0x10,
                            offset: 0,
                        },
                        limits: FunctionLimits::default(),
                    },
                )
                .unwrap();
            let code = compiler.recv().await.unwrap().unwrap();
            assert_eq!(
                code.blocks()
                    .iter()
                    .map(|b| (b.start_eip, b.byte_len))
                    .collect::<Vec<_>>(),
                [(0, 3), (3, 3), (6, 3), (9, 5)]
            );
            let exit = runner.cpu().exit_handle();
            runner
                .cpu()
                .install([Hook::host(0x120, async move || exit.request())])
                .await
                .unwrap();
            code.install(runner.cpu()).unwrap();
            assert_eq!(
                runner.run_cpu().await,
                ExecutionExit::Host(HostExit::Return)
            );
            let state = runner.cpu().cpu_state();
            assert_eq!(state.ax(), 42);
            assert_eq!(state.cx(), 0);
            assert_eq!(state.dx(), 1);
            assert_eq!(state.sp(), 0x800);
            assert_eq!(state.instruction_count(), 10);
        })
        .await;
    machine.shutdown().await.unwrap();
    assert_eq!(result.unwrap(), Some(()));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn discovered_function_runs_branches_and_an_interpreted_callee() {
    pollster::block_on(compilation::run(execute_function));
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn discovered_function_runs_branches_and_an_interpreted_callee() {
    compilation::run(execute_function).await;
}
