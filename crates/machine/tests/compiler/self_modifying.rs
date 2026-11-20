use super::*;
use relocade_machine::{DiscoveryLimits, FunctionRequest};

async fn patch_running_code(compiler: &relocade_machine::CompilerService) {
    let map =
        PhysicalMemoryMap::new([(0..=0x3fff, PhysicalMapping::Ram { backing_offset: 0 })]).unwrap();
    let mut machine = Machine::new(MachineConfig::new(0x4000, map).unwrap()).unwrap();
    let config = CpuConfig::new(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState::real_mode(),
    )
    .unwrap();
    let memory = machine.memory();
    let mut view = memory.view();
    // CALL 0FFB; JMP 4000. The stack is on its own page so the compiled CALL uses its link.
    view.write(0x100, &[0xe8, 0xf8, 0x0e, 0xe9, 0xfa, 0x3e])
        .unwrap();
    // MOV AX,7; ADD AX,1; RET. The ADD immediate spans two physical pages.
    view.write(0xffb, &[0xb8, 7, 0, 0x05, 1, 0, 0xc3]).unwrap();
    // LOCK INC word [0FFF]; JMP BX. BX points at the compiled caller.
    view.write(0x200, &[0xf0, 0xff, 6, 0xff, 0x0f, 0xff, 0xe3])
        .unwrap();
    // REP MOVSB patches the immediately following MOV AX,7; JMP 4000.
    view.write(0x300, &[0xf3, 0xa4, 0xb8, 7, 0, 0xe9, 0xf8, 0x3c])
        .unwrap();
    view.write(0x500, &[0x34, 0x12]).unwrap();

    let cpu = machine.create_cpu(config, Some(compiler)).unwrap();
    cpu.run_until(pending::<()>(), async move |mut runner| {
        let mut compiler = runner.take_compiler().unwrap();
        let state = runner.cpu().cpu_state();
        for offset in [0x100, 0xffb, 0x200, 0x300] {
            compiler
                .request_functions(
                    state.execution_target(),
                    vec![FunctionRequest {
                        entry: FarPointer { segment: 0, offset },
                        limits: DiscoveryLimits::default(),
                    }],
                )
                .unwrap();
            let code = compiler.recv().await.unwrap().unwrap();
            assert!(code
                .blocks()
                .iter()
                .any(|b| b.address.eip() == u32::from(offset)));
            code.install(runner.cpu()).unwrap();
        }
        let exit = runner.cpu().exit_handle();
        runner
            .cpu()
            .install([Hook::host(0x4000, async move || exit.request())])
            .await
            .unwrap();

        state.set_sp(0x3800);
        state.set_bx(0x100);
        state.set_si(0x500);
        state.set_di(0x303);
        state.set_cx(2);
        for (ip, ax, instructions) in [(0x100, 8, 5), (0x200, 9, 7), (0x300, 0x1234, 3)] {
            let before = state.instruction_count();
            state.set_eip(ip);
            assert_eq!(
                runner.run_cpu().await,
                ExecutionExit::Host(HostExit::Return)
            );
            assert_eq!(state.ax(), ax);
            assert_eq!(state.instruction_count() - before, instructions);
            assert_eq!(state.sp(), 0x3800);
        }

        assert_eq!(runner.memory().view().read_u16(0xfff).unwrap(), 2);
        assert_eq!((state.si(), state.di(), state.cx()), (0x502, 0x305, 0));
        assert_eq!(runner.cpu().compiled_modules_at(0xffb).count(), 0);
        assert_eq!(runner.cpu().compiled_modules_at(0x300).count(), 0);
        assert_eq!(runner.cpu().compiled_modules_at(0x100).count(), 1);
        assert_eq!(runner.cpu().compiled_modules_at(0x200).count(), 1);
    })
    .await
    .unwrap()
    .unwrap();
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn guest_writes_retire_linked_and_executing_code() {
    pollster::block_on(compilation::run(patch_running_code));
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn guest_writes_retire_linked_and_executing_code() {
    compilation::run(patch_running_code).await;
}
