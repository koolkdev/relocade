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
                .request_functions(
                    target,
                    vec![FunctionRequest {
                        entry: FarPointer {
                            segment: 0x10,
                            offset: 0,
                        },
                        limits: FunctionLimits::default(),
                    }],
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

async fn execute_batch(compiler: &relocade_machine::CompilerService) {
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
                0xb8, 42, 0, // MOV AX,42
                0x9a, 0, 0, 0x20, 0, // CALL FAR 0020:0000
                0xff, 0xe3, // JMP BX to the host exit
            ],
        )
        .unwrap();
    machine.memory().view().write(0x200, &[0x42, 0xcb]).unwrap(); // INC DX; RETF
    machine.memory().view().write(0x300, &[0xf1]).unwrap(); // Unsupported candidate
    let cpu = machine.create_cpu(config, Some(compiler)).unwrap();
    let result = cpu
        .run_until(pending::<()>(), async move |mut runner| {
            let target = runner.cpu().cpu_state().execution_target();
            let mut compiler = runner.take_compiler().unwrap();
            let request = |segment| FunctionRequest {
                entry: FarPointer { segment, offset: 0 },
                limits: FunctionLimits::default(),
            };
            // The failed candidate between the valid functions must not discard either.
            compiler
                .request_functions(target, vec![request(0x10), request(0x30), request(0x20)])
                .unwrap();
            let code = compiler.recv().await.unwrap().unwrap();
            assert_eq!(
                code.blocks()
                    .iter()
                    .map(|block| (block.start_eip, block.byte_len))
                    .collect::<Vec<_>>(),
                [(0, 8), (8, 2), (0, 2)]
            );
            compiler
                .request_functions(target, vec![request(0x30)])
                .unwrap();
            assert!(matches!(
                compiler.recv().await.unwrap(),
                Err(relocade_machine::Error::Compile(_))
            ));
            let exit = runner.cpu().exit_handle();
            runner
                .cpu()
                .install([
                    Hook::host(0x100, async || panic!("caller was not installed")),
                    Hook::host(0x200, async || panic!("callee was not installed")),
                    Hook::host(0x120, async move || exit.request()),
                ])
                .await
                .unwrap();
            code.install(runner.cpu()).unwrap();
            assert_eq!(
                runner.run_cpu().await,
                ExecutionExit::Host(HostExit::Return)
            );
            let state = runner.cpu().cpu_state();
            assert_eq!((state.ax(), state.dx(), state.sp()), (42, 1, 0x800));
            assert_eq!(state.instruction_count(), 5);
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

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn function_batch_shares_a_module_across_segments_and_discovery_failures() {
    pollster::block_on(compilation::run(execute_batch));
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn function_batch_shares_a_module_across_segments_and_discovery_failures() {
    compilation::run(execute_batch).await;
}
