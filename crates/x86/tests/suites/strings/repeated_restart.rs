//! Reuse the emitted REP block with the architectural state left by a repaired fault.
use super::record;
use crate::support::{
    guest::{
        Exit, Machine,
        Permissions::{ReadOnly, ReadWrite},
    },
    step::{Engine, TestModule},
};
use wasm86_x86::compile_block_from_bytes;

fn restart_after_source_page_repair(engine: Engine) {
    let code = [0xf3, 0xa4];
    let block = TestModule::new(&compile_block_from_bytes(0x1000, &code, 1).unwrap());
    for module in [TestModule::interpreter(), &block] {
        let mut machine = Machine::new(&code);
        machine.cpu.flags = record(0xfe);
        machine.cpu.instruction_count = 17;
        machine.cpu.registers.ecx = 4;
        machine.cpu.registers.esi = 0x4ffe;
        machine.cpu.registers.edi = 0x7000;
        machine.memory(0x4ffe, &[0x12, 0x34], ReadOnly);
        machine.memory(0x7000, &[0xa5; 4], ReadWrite);
        let initial = machine.state();
        let fault = machine.run(module, engine);
        let mut expected = initial;
        expected.cpu.registers.ecx = 2;
        expected.cpu.registers.esi = 0x5000;
        expected.cpu.registers.edi = 0x7002;
        expected.memory.write(0x7000, &[0x12, 0x34]);
        assert_eq!(fault.state, expected);
        assert_eq!(
            fault.exit,
            Exit::PageFault {
                address: 0x5000,
                error: 0
            }
        );
        assert!(fault.dispatches.is_empty() && fault.machine_unchanged);

        // Resume from the actual architectural state, with the absent page repaired.
        // Alter already-consumed source bytes so restarting the entire copy is observable.
        machine.cpu = fault.state.cpu;
        machine.memory(0x4ffe, &[0x99, 0x99], ReadOnly);
        machine.memory(0x5000, &[0x56, 0x78], ReadOnly);
        machine.memory(0x7000, &fault.state.memory.read(0x7000, 4), ReadWrite);
        let mut completed = machine.state();
        completed.cpu.registers.ecx = 0;
        completed.cpu.registers.esi = 0x5002;
        completed.cpu.registers.edi = 0x7004;
        completed.cpu.eip = 0x1002;
        completed.cpu.instruction_count = 18;
        completed.memory.write(0x7000, &[0x12, 0x34, 0x56, 0x78]);
        let actual = machine.run(module, engine);
        assert_eq!(actual.state, completed);
        assert_eq!(actual.exit, Exit::Dispatch(0x1002));
        assert_eq!(actual.dispatches, [(0x1002, completed)]);
        assert!(actual.machine_unchanged);
    }
}

#[test]
fn repaired_rep_resumes_after_completed_elements() {
    restart_after_source_page_repair(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn repaired_rep_resumes_after_completed_elements_v8() {
    restart_after_source_page_repair(Engine::V8);
}

fn mixed_width_state_before_rep_fault() -> Vec<crate::support::sequences::SequenceCase> {
    use crate::support::sequences::{Checkpoint as Step, SequenceCase as Case};
    use wasm86_x86::Gpr32::{Eax, Ecx, Edi, Esi};
    [1u32, 2, 4]
        .into_iter()
        .map(|width| {
            let code = super::Operation::Stos.code(width);
            let code = [vec![0xf3], code].concat();
            let start = 0x8000 - 2 * width;
            let expected = [0x12, 0x56, 0xbb, 0xaa][..width as usize].repeat(2);
            Case::preserving_flags(format!(
                "mixed AX/AH/CX definitions survive REP STOS width {width} progress fault"
            ))
            .stored_flags(record(0xfe))
            .instruction_count(u32::MAX - 1)
            .initial_registers(&[
                (Eax, 0xaabb_ccdd),
                (Ecx, 0x0001_00aa),
                (Edi, start),
                (Esi, 0x9000),
            ])
            .memory(start, &vec![0xa5; (width * 2) as usize], ReadWrite)
            .step(Step::preserving_flags(&[0x66, 0xb8, 0x12, 0x34]).register(Eax, 0xaabb_3412))
            .step(Step::preserving_flags(&[0xb4, 0x56]).register(Eax, 0xaabb_5612))
            .step(Step::preserving_flags(&[0x66, 0xb9, 3, 0]).register(Ecx, 0x0001_0003))
            .step(
                Step::preserving_flags(&code)
                    .register(Ecx, 0x0001_0001)
                    .register(Edi, 0x8000)
                    .expect_memory(start, &expected)
                    .fault(0x8000, 2),
            )
        })
        .collect()
}

crate::support::sequences::test_sequences!(
    mixed_width_prior_state_survives_repeat_fault,
    mixed_width_state_before_rep_fault()
);
