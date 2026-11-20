use super::*;
use relocade_machine::{DiscoveryLimits, Error, FunctionRequest, Runner};

async fn run_from_start(runner: &mut Runner) -> u16 {
    let state = runner.cpu().cpu_state();
    state.set_eip(0x100);
    state.set_bx(0x1000);
    assert_eq!(
        runner.run_cpu().await,
        ExecutionExit::Host(HostExit::Return)
    );
    runner.cpu().cpu_state().ax()
}

async fn patch_jump(compiler: &relocade_machine::CompilerService) {
    let (mut machine, config) = setup();
    let memory = machine.memory();
    let mut view = memory.view();
    view.write(0x100, &[0xb8, 1, 0, 0xe9, 0x0a, 0]).unwrap(); // MOV AX,1; JMP 0110
    view.write(0x110, &[0x83, 0xc0, 2, 0xff, 0xe3]).unwrap(); // ADD AX,2; JMP BX
    view.write(0x120, &[0x83, 0xc0, 4, 0xff, 0xe3]).unwrap(); // ADD AX,4; JMP BX
    view.write(0x220, &[0x83, 0xc0, 8, 0xff, 0xe3]).unwrap(); // ADD AX,8; JMP BX

    let cpu = machine.create_cpu(config, Some(compiler)).unwrap();
    cpu.run_until(pending::<()>(), async move |mut runner| {
        let exit = runner.cpu().exit_handle();
        runner
            .cpu()
            .install([Hook::host(0x1000, async move || exit.request())])
            .await
            .unwrap();
        let target = runner.cpu().cpu_state().execution_target();
        let mut compiler = runner.take_compiler().unwrap();
        let mut compile = async |entry, limits| {
            compiler
                .request_functions(target, vec![FunctionRequest { entry, limits }])
                .unwrap();
            compiler.recv().await.unwrap().unwrap()
        };
        let root = FarPointer {
            segment: 0,
            offset: 0x100,
        };
        let limits = DiscoveryLimits::default();
        let late = compile(root, limits).await;
        compile(root, limits).await.install(runner.cpu()).unwrap();
        assert_eq!(run_from_start(&mut runner).await, 3);

        // Two writes to one installed version use the same retry.
        memory.view().write_u8(0x10_0104, 0x0c).unwrap();
        memory.view().write_u8(0x10_0104, 0x1a).unwrap();
        assert_eq!(run_from_start(&mut runner).await, 5);
        let retry = compile(root, limits).await;
        assert_eq!(retry.ranges(), [0x100..0x106, 0x120..0x125]);
        retry.install(runner.cpu()).unwrap();
        assert_eq!(run_from_start(&mut runner).await, 5);

        // This write changes the other byte of the same jump's displacement.
        memory.view().write_u8(0x105, 1).unwrap();
        assert_eq!(run_from_start(&mut runner).await, 9);
        assert_eq!(
            compile(root, limits).await.ranges(),
            [0x100..0x103, 0x220..0x225]
        );

        // Restore the exact bytes that the pending reply captured.
        memory.view().write(0x104, &[0x0a, 0]).unwrap();
        assert!(matches!(
            late.install(runner.cpu()),
            Err(Error::StaleCompilation)
        ));
        assert_eq!(run_from_start(&mut runner).await, 3);

        // A different CS:IP and a different block split reach the same backing byte.
        let alias = FarPointer {
            segment: 0x10,
            offset: 0,
        };
        let split = DiscoveryLimits {
            block_instructions: 1,
            ..limits
        };
        assert_eq!(
            compile(alias, split).await.ranges(),
            [0x100..0x103, 0x110..0x115]
        );
        let stable = compile(root, limits).await;
        assert_eq!(stable.ranges(), [0x100..0x103, 0x110..0x115]);
        assert!(stable
            .blocks()
            .iter()
            .any(|entry| entry.address.eip() == 0x110));
        stable.install(runner.cpu()).unwrap();
        assert_eq!(run_from_start(&mut runner).await, 3);

        // The interpreted jump observes further edits while the stable code stays installed.
        memory.view().write_u8(0x10_0104, 0x1a).unwrap();
        assert_eq!(run_from_start(&mut runner).await, 5);
        assert_eq!(runner.cpu().compiled_modules_at(0x100).count(), 1);
        assert_eq!(runner.cpu().compiled_modules_at(0x110).count(), 1);

        compiler
            .request(
                target,
                vec![BlockRequest {
                    entry: FarPointer {
                        segment: 0x10,
                        offset: 3,
                    },
                    byte_len: 3,
                    instruction_limit: 1,
                }],
            )
            .unwrap();
        let excluded = compiler.recv().await.unwrap().unwrap();
        assert!(excluded.ranges().is_empty());
        assert!(excluded.blocks().is_empty());
    })
    .await
    .unwrap()
    .unwrap();
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn repeated_jump_patches_preserve_stable_instructions() {
    pollster::block_on(compilation::run(patch_jump));
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn repeated_jump_patches_preserve_stable_instructions() {
    compilation::run(patch_jump).await;
}
