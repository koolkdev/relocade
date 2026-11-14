use super::*;
use relocade_machine::{FarPointer, FunctionRequest, Runner};
use relocade_x86::FunctionLimits;

async fn run_caller(runner: &mut Runner, expected: u16) {
    let state = runner.cpu().cpu_state();
    state.set_eip(0x100);
    state.set_ax(0);
    state.set_sp(0x800);
    assert_eq!(
        runner.run_cpu().await,
        ExecutionExit::Host(HostExit::Return)
    );
    let state = runner.cpu().cpu_state();
    assert_eq!(state.ax(), expected);
    assert_eq!(state.sp(), 0x800);
}

async fn retire_linked_compilations(compiler: &relocade_machine::CompilerService) {
    let (mut machine, config) = setup();
    let memory = machine.memory();
    let mut memory = memory.view();
    memory
        .write(0x100, &[0xe8, 0xfd, 0, 0xe9, 0xfa, 0x0e]) // CALL 0200; JMP 1000
        .unwrap();
    memory
        .write(0x200, &[0xb8, 7, 0, 0xe9, 0x0a, 0]) // MOV AX,7; JMP 0210
        .unwrap();
    memory.write(0x210, &[0x83, 0xc0, 1, 0xc3]).unwrap(); // ADD AX,1; RET
    let cpu = machine.create_cpu(config, Some(compiler)).unwrap();
    cpu.run_until(pending::<()>(), async move |mut runner| {
        let target = runner.cpu().cpu_state().execution_target();
        let mut compiler = runner.take_compiler().unwrap();
        let request = |offset| FunctionRequest {
            entry: FarPointer { segment: 0, offset },
            limits: FunctionLimits::default(),
        };
        let exit = runner.cpu().exit_handle();
        runner
            .cpu()
            .install([Hook::host(0x1000, async move || exit.request())])
            .await
            .unwrap();
        // Separate modules exercise the caller's table and the callee's direct jump.
        for entry in [0x100, 0x200] {
            compiler
                .request_functions(target, vec![request(entry)])
                .unwrap();
            compiler
                .recv()
                .await
                .unwrap()
                .unwrap()
                .install(runner.cpu())
                .unwrap();
        }
        run_caller(&mut runner, 8).await;

        // Read-ahead and the gap before the branch target are not source dependencies.
        runner.cpu().invalidate_code(0x206..0x210).unwrap();
        assert_eq!(runner.cpu().compiled_modules_at(0x200).count(), 1);

        runner.cpu().invalidate_code(0x212..0x213).unwrap();
        assert_eq!(runner.cpu().compiled_modules_at(0x200).count(), 0);
        assert_eq!(runner.cpu().compiled_modules_at(0x210).count(), 0);
        assert_eq!(runner.cpu().compiled_modules_at(0x100).count(), 1);
        runner.cpu().memory().view().write(0x212, &[2]).unwrap();
        run_caller(&mut runner, 9).await;

        // Incoming calls must also reconnect when a replacement arrives.
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
        run_caller(&mut runner, 9).await;

        // Replace only the root with a smaller compilation that does not depend
        // on the tail. Invalidating the old tail must preserve this newer root.
        compiler
            .request(
                target,
                vec![BlockRequest {
                    address: 0x200,
                    start_eip: 0x200,
                    byte_len: 3,
                    instruction_limit: 1,
                }],
            )
            .unwrap();
        compiler
            .recv()
            .await
            .unwrap()
            .unwrap()
            .install(runner.cpu())
            .unwrap();
        assert_eq!(runner.cpu().compiled_modules_at(0x200).count(), 2);
        runner.cpu().invalidate_code(0x210..0x214).unwrap();
        assert_eq!(runner.cpu().compiled_modules_at(0x200).count(), 1);
        assert_eq!(runner.cpu().compiled_modules_at(0x210).count(), 0);
        runner.cpu().memory().view().write(0x212, &[3]).unwrap();
        run_caller(&mut runner, 10).await;

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
        let exit = runner.cpu().exit_handle();
        runner
            .cpu()
            .install([Hook::host(0x200, async move || exit.request())])
            .await
            .unwrap();
        runner.cpu().invalidate_code(0x200..0x214).unwrap();
        assert_eq!(runner.cpu().compiled_modules_at(0x200).count(), 0);
        let state = runner.cpu().cpu_state();
        state.set_eip(0x100);
        state.set_sp(0x800);
        assert_eq!(
            runner.run_cpu().await,
            ExecutionExit::Host(HostExit::Return)
        );
        let state = runner.cpu().cpu_state();
        assert_eq!(state.eip(), 0x200);
        assert_eq!(state.sp(), 0x7fe);

        // Retired callers must leave no reverse links to missing module instances.
        runner.cpu().invalidate_code(0x100..0x106).unwrap();
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
        run_caller(&mut runner, 10).await;
    })
    .await
    .unwrap()
    .unwrap();
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn retirement_repairs_links_and_preserves_newer_entries() {
    pollster::block_on(compilation::run(retire_linked_compilations));
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn retirement_repairs_links_and_preserves_newer_entries() {
    compilation::run(retire_linked_compilations).await;
}

async fn reject_changed_source(compiler: &relocade_machine::CompilerService) {
    let (mut machine, config) = setup();
    machine
        .memory()
        .view()
        .write(0x100, &[0xb8, 42, 0, 0xeb, 0])
        .unwrap();
    let cpu = machine.create_cpu(config, Some(compiler)).unwrap();
    cpu.run_until(pending::<()>(), async move |mut runner| {
        let target = runner.cpu().cpu_state().execution_target();
        let mut compiler = runner.take_compiler().unwrap();
        compiler.request(target, vec![request()]).unwrap();
        let code = compiler.recv().await.unwrap().unwrap();
        // The reader may copy a page, but unused bytes are not dependencies.
        runner.cpu().memory().view().write(0x108, &[0x90]).unwrap();
        code.install(runner.cpu()).unwrap();
        runner.cpu().invalidate_code(0x100..0x105).unwrap();
        // A20's high alias writes the same instruction backing.
        runner.cpu().memory().view().write(0x100101, &[73]).unwrap();
        assert!(matches!(
            code.install(runner.cpu()),
            Err(relocade_machine::Error::StaleCompilation)
        ));
        assert_eq!(runner.cpu().compiled_modules_at(0x100).count(), 0);
        compiler.request(target, vec![request()]).unwrap();
        let fresh = compiler.recv().await.unwrap().unwrap();
        fresh.install(runner.cpu()).unwrap();
        let exit = runner.cpu().exit_handle();
        runner
            .cpu()
            .install([Hook::host(0x105, async move || exit.request())])
            .await
            .unwrap();
        assert_eq!(
            runner.run_cpu().await,
            ExecutionExit::Host(HostExit::Return)
        );
        assert_eq!(runner.cpu().cpu_state().ax(), 73);
    })
    .await
    .unwrap()
    .unwrap();
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn installation_rejects_changed_instruction_bytes() {
    pollster::block_on(compilation::run(reject_changed_source));
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn installation_rejects_changed_instruction_bytes() {
    compilation::run(reject_changed_source).await;
}

async fn self_modifying_blocks(compiler: &relocade_machine::CompilerService) {
    for (program, expected_ax, expected_bx) in [
        // MOV byte [0106],42; MOV AX,7; JMP 1000.
        (vec![0xc6, 6, 6, 1, 42, 0xb8, 7, 0, 0xe9, 0xf5, 0x0e], 42, 0),
        // MOV AL,42; MOV DI,010B; MOV CX,1; REP STOSB; MOV BX,7; JMP 1000.
        (
            vec![
                0xb0, 42, 0xbf, 0x0b, 1, 0xb9, 1, 0, 0xf3, 0xaa, 0xbb, 7, 0, 0xe9, 0xf0, 0x0e,
            ],
            42,
            42,
        ),
    ] {
        let (mut machine, config) = setup();
        machine.memory().view().write(0x100, &program).unwrap();
        let cpu = machine.create_cpu(config, Some(compiler)).unwrap();
        cpu.run_until(pending::<()>(), async move |mut runner| {
            let target = runner.cpu().cpu_state().execution_target();
            let mut compiler = runner.take_compiler().unwrap();
            compiler
                .request_functions(
                    target,
                    vec![FunctionRequest {
                        entry: FarPointer {
                            segment: 0,
                            offset: 0x100,
                        },
                        limits: FunctionLimits::default(),
                    }],
                )
                .unwrap();
            let code = compiler.recv().await.unwrap().unwrap();
            code.install(runner.cpu()).unwrap();
            let exit = runner.cpu().exit_handle();
            runner
                .cpu()
                .install([Hook::host(0x1000, async move || exit.request())])
                .await
                .unwrap();
            assert_eq!(
                runner.run_cpu().await,
                ExecutionExit::Host(HostExit::Return)
            );
            assert_eq!(runner.cpu().cpu_state().ax(), expected_ax);
            assert_eq!(runner.cpu().cpu_state().bx(), expected_bx);
            assert_eq!(runner.cpu().compiled_modules_at(0x100).count(), 0);
        })
        .await
        .unwrap()
        .unwrap();
        machine.shutdown().await.unwrap();
    }
}

async fn tracked_writes_repair_links(compiler: &relocade_machine::CompilerService) {
    let (mut machine, config) = setup();
    let memory = machine.memory();
    memory
        .view()
        .write(0x100, &[0xe8, 0xfd, 0, 0xe9, 0xfa, 0x0e])
        .unwrap();
    memory
        .view()
        .write(0x200, &[0xb8, 7, 0, 0xe9, 0x0a, 0])
        .unwrap();
    memory.view().write(0x210, &[0x83, 0xc0, 1, 0xc3]).unwrap();
    let cpu = machine.create_cpu(config, Some(compiler)).unwrap();
    cpu.run_until(pending::<()>(), async move |mut runner| {
        let target = runner.cpu().cpu_state().execution_target();
        let mut compiler = runner.take_compiler().unwrap();
        let request = |offset| FunctionRequest {
            entry: FarPointer { segment: 0, offset },
            limits: FunctionLimits::default(),
        };
        let exit = runner.cpu().exit_handle();
        runner
            .cpu()
            .install([Hook::host(0x1000, async move || exit.request())])
            .await
            .unwrap();
        for entry in [0x100, 0x200] {
            compiler
                .request_functions(target, vec![request(entry)])
                .unwrap();
            compiler
                .recv()
                .await
                .unwrap()
                .unwrap()
                .install(runner.cpu())
                .unwrap();
        }
        run_caller(&mut runner, 8).await;
        // Unrelated data and identical instruction writes do not retire a version.
        runner.cpu().memory().view().write(0x208, &[99]).unwrap();
        runner.cpu().memory().view().write(0x212, &[1]).unwrap();
        run_caller(&mut runner, 8).await;
        assert_eq!(runner.cpu().compiled_modules_at(0x200).count(), 1);
        // The source is at backing 1212, reached here through an A20 alias.
        runner.cpu().memory().view().write(0x100212, &[2]).unwrap();
        run_caller(&mut runner, 9).await;
        assert_eq!(runner.cpu().compiled_modules_at(0x200).count(), 0);
        assert_eq!(runner.cpu().compiled_modules_at(0x100).count(), 1);
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
        run_caller(&mut runner, 9).await;
        // Direct backing writes, including firmware/device initialization, are tracked too.
        runner
            .cpu()
            .memory()
            .view()
            .write_backing(0x1212, &[3])
            .unwrap();
        run_caller(&mut runner, 10).await;
        assert_eq!(runner.cpu().compiled_modules_at(0x200).count(), 0);
    })
    .await
    .unwrap()
    .unwrap();
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn stores_to_live_code_restart_before_current_instruction_effects() {
    pollster::block_on(compilation::run(self_modifying_blocks));
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn stores_to_live_code_restart_before_current_instruction_effects() {
    compilation::run(self_modifying_blocks).await;
}
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn host_writes_retire_source_owners_and_repair_incoming_links() {
    pollster::block_on(compilation::run(tracked_writes_repair_links));
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn host_writes_retire_source_owners_and_repair_incoming_links() {
    compilation::run(tracked_writes_repair_links).await;
}
