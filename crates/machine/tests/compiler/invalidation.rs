use super::*;
use relocade_machine::{DiscoveryLimits, FarPointer, FunctionRequest, Runner};
use std::sync::Arc;

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
            limits: DiscoveryLimits::default(),
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
            let code = compiler.recv().await.unwrap().unwrap();
            assert!(runner
                .memory()
                .installed_code_at(0x1000 + u32::from(entry))
                .is_empty());
            code.install(runner.cpu()).unwrap();
        }
        // Installations retain shared dependencies after the reply is dropped.
        let sources = runner.cpu().memory();
        let callee = sources.installed_code_at(0x1212).pop().unwrap();
        let caller = sources.installed_code_at(0x1100).pop().unwrap();
        assert!(sources.installed_code_at(0x1206).is_empty());
        run_caller(&mut runner, 8).await;
        // CALL pushes onto the same backing page as both installations.
        assert!(!callee.was_modified());
        assert!(!caller.was_modified());

        // Read-ahead and the gap before the branch target are not source dependencies.
        runner.cpu().invalidate_code(0x206..0x210).unwrap();
        assert_eq!(runner.cpu().compiled_modules_at(0x200).count(), 1);

        // Single-step MOV byte [0212],2, then retire before entering the changed code.
        sources
            .view()
            .write(0x400, &[0xc6, 0x06, 0x12, 0x02, 2])
            .unwrap();
        runner.cpu().cpu_state().set_eip(0x400);
        assert_eq!(runner.cpu().step(), ExecutionExit::StepComplete);
        assert!(callee.was_modified());
        assert!(!caller.was_modified());
        assert_eq!(sources.view().read_u8(0x212).unwrap(), 2);
        runner.cpu().invalidate_code(0x212..0x213).unwrap();
        assert_eq!(runner.cpu().compiled_modules_at(0x200).count(), 0);
        assert_eq!(runner.cpu().compiled_modules_at(0x210).count(), 0);
        assert_eq!(runner.cpu().compiled_modules_at(0x100).count(), 1);
        assert!(sources.installed_code_at(0x1212).is_empty());
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
                    entry: FarPointer {
                        segment: 0,
                        offset: 0x200,
                    },
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

async fn installation_lifetimes(compiler: &relocade_machine::CompilerService) {
    let (mut machine, config) = setup();
    machine
        .memory()
        .view()
        .write(0x100, &[0xb8, 0x34, 0x12, 0x90, 0x90])
        .unwrap();
    let cpu = machine.create_cpu(config, Some(compiler)).unwrap();
    cpu.run_until(pending::<()>(), async move |mut runner| {
        let memory = runner.memory();
        let target = runner.cpu().cpu_state().execution_target();
        let mut compiler = runner.take_compiler().unwrap();
        compiler
            .request(
                target,
                vec![BlockRequest {
                    instruction_limit: 1,
                    ..request()
                }],
            )
            .unwrap();
        let code = compiler.recv().await.unwrap().unwrap();
        let pending_clone = code.clone();
        assert!(memory.installed_code_at(0x1101).is_empty());

        code.install(runner.cpu()).unwrap();
        let first = memory.installed_code_at(0x1101).pop().unwrap();
        assert!(first
            .source()
            .backing_ranges()
            .eq(std::iter::once(0x1100..0x1103)));
        assert!(memory.installed_code_at(0x1103).is_empty());
        // Change only the read-ahead byte, then change and restore an instruction byte.
        memory.view().write(0x102, &[0x12, 0xcc]).unwrap();
        assert!(!first.was_modified());
        memory.view().write_u8(0x101, 0x35).unwrap();
        memory.view().write_u8(0x101, 0x34).unwrap();
        assert!(first.was_modified());

        pending_clone.install(runner.cpu()).unwrap();
        let active = memory.installed_code_at(0x1101);
        assert_eq!(active.len(), 2);
        assert!(active.iter().any(|item| Arc::ptr_eq(item, &first)));
        let second = active
            .iter()
            .find(|item| !Arc::ptr_eq(item, &first))
            .unwrap();
        assert!(!second.was_modified());

        runner.cpu().invalidate_code(0x101..0x102).unwrap();
        assert!(memory.installed_code_at(0x1101).is_empty());
        code.install(runner.cpu()).unwrap();
        let last = memory.installed_code_at(0x1101).pop().unwrap();
        drop(runner);
        assert!(memory.installed_code_at(0x1101).is_empty());

        memory.view().write_u8(0x101, 0xff).unwrap();
        assert!(first.was_modified());
        assert!(!second.was_modified());
        assert!(!last.was_modified());
        assert!(last.source().ranges().eq(pending_clone.source().ranges()));
    })
    .await
    .unwrap()
    .unwrap();
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn source_registrations_follow_each_installation() {
    pollster::block_on(compilation::run(installation_lifetimes));
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn source_registrations_follow_each_installation() {
    compilation::run(installation_lifetimes).await;
}
