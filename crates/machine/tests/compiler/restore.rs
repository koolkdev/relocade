use super::*;
use relocade_x86::{Segment, SegmentDefaultSize::Bits32, SegmentRegister};

async fn restore_with_compiled_code(service: &relocade_machine::CompilerService) {
    let (mut machine, _) = setup();
    let mut protected = CpuState::protected_mode(Bits32, Bits32);
    let entry = FarPointer {
        segment: protected.segments.cs.selector,
        offset: 0x100,
    };
    let mut real = CpuState::real_mode();
    real.segments.cs = SegmentRegister::real_mode(Segment::Cs, entry.segment);
    real.eip = u32::from(entry.offset);
    protected.segments.cs.base = real.segments.cs.base;
    protected.eip = real.eip;
    protected.registers.esp = 0x800;
    let memory = machine.memory();
    // CALL rel32 to the following instruction.
    memory
        .view()
        .write(entry.address() - 5, &[0xe8, 0, 0, 0, 0])
        .unwrap();
    // Real16 decodes MOV AX,1234 then JS +56. Protected32 decodes MOV EAX,56781234.
    // F1 is unsupported and stops execution without retiring.
    memory
        .view()
        .write(entry.address(), &[0xb8, 0x34, 0x12, 0x78, 0x56, 0xf1])
        .unwrap();
    let cpu = machine
        .create_cpu(CpuConfig::pc(real).unwrap(), Some(service))
        .unwrap();
    ExecutionWorker::spawn(cpu, SessionConfig::host(), async move |mut runner| {
        let state = runner.cpu().cpu_state();
        let mut compiler = runner.take_compiler().unwrap();
        let block = BlockRequest {
            entry,
            byte_len: 3,
            instruction_limit: 1,
        };
        compiler
            .request(state.compiler_config(), vec![block])
            .unwrap();
        let code = compiler.recv().await.unwrap().unwrap();
        code.install(runner.cpu()).unwrap();
        // The compiled block ends after MOV. The interpreter's first checkpoint is after JS.
        assert_eq!(
            support::checkpoint(runner.cpu()).await,
            ExecutionExit::Interrupted
        );
        assert_eq!(
            (state.eip(), state.ax(), state.instruction_count()),
            (real.eip + 3, 0x1234, 1)
        );

        compiler
            .request(state.compiler_config(), vec![block])
            .unwrap();
        state.restore(&protected).unwrap();
        let before = state.instruction_count();
        let end = ExecutionExit::UnsupportedInstruction {
            eip: protected.eip + 5,
        };
        assert_eq!(runner.run_cpu().await, end);
        assert_eq!(state.eax(), 0x5678_1234);
        assert_eq!(state.eip(), protected.eip + 5);
        assert_eq!(state.instruction_count(), before + 1);

        // A late Real16 reply binds the same CS:IP while the CPU is still protected.
        compiler
            .recv()
            .await
            .unwrap()
            .unwrap()
            .install(runner.cpu())
            .unwrap();
        state.set_eip(protected.eip - 5);
        state.set_eax(0);
        let before = state.instruction_count();
        assert_eq!(runner.run_cpu().await, end);
        assert_eq!(state.eax(), 0x5678_1234);
        assert_eq!(state.instruction_count(), before + 2);
        assert_eq!(state.esp(), 0x7fc);
        assert_eq!(
            runner.memory().view().read_u32(0x7fc).unwrap(),
            protected.eip
        );
    })
    .join()
    .await
    .unwrap()
    .unwrap();
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn real16_bindings_are_inaccessible_after_protected_restore() {
    pollster::block_on(compilation::run(restore_with_compiled_code));
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn real16_bindings_are_inaccessible_after_protected_restore() {
    compilation::run(restore_with_compiled_code).await;
}
