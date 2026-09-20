use super::*;
use crate::support::{
    guest::{Exit, Machine, Permissions::ReadOnly},
    sequences::{test_sequences, Checkpoint as Step, SequenceCase},
    step::{Argument, Engine, Event, Input, Outcome, TestModule},
};
use wasm86_x86::{
    compile_block_from_bytes,
    Gpr32::{Eax, Ebx, Ecx, Edi, Esi},
};

fn prior_changes() -> Vec<SequenceCase> {
    let mut cases = Vec::new();
    for operation in COMPARISONS {
        for prefix in [0xf2, 0xf3] {
            let mut case = SequenceCase::from_opaque_flags(format!("{operation:?} {prefix:02x} fault retains pending arithmetic, STC and narrow registers"))
                .stored_flags(record(0xfe)).instruction_count(u32::MAX - 1)
                .initial_registers(&[(Eax, 0xabcd_3405), (Ebx, 0x7fff_ffff), (Ecx, 0xaaaa_dead), (Edi, 0xbbbb_7ffe), (Esi, 0xcccc_4000)])
                .memory(0x7ffe, &[if prefix == 0xf3 { 5 } else { 3 }; 2], ReadOnly)
                .step(Step::new(&[0x83, 0xc3, 1], flags(54)).register(Ebx, 0x8000_0000))
                .step(Step::new(&[0xf9], flags(55)))
                .step(Step::preserving_flags(&[0x66, 0xb9, 3, 0]).register(Ecx, 0xaaaa_0003))
                .step(Step::preserving_flags(&[0xb4, 0x12]).register(Eax, 0xabcd_1205));
            let mut fault = Step::preserving_flags(&code(operation, prefix, 1, true, false))
                .register(Ecx, 0xaaaa_0001)
                .register(Edi, 0xbbbb_8000)
                .fault(0x8000, 0);
            if operation == Operation::Cmps {
                case = case.memory(0x4000, &[5; 3], ReadOnly);
                fault = fault.register(Esi, 0xcccc_4002);
            }
            cases.push(case.step(fault));
        }
    }
    cases
}

test_sequences!(
    faults_restore_instruction_entry_flags_after_prior_changes,
    prior_changes()
);

fn repaired(engine: Engine) {
    let observer = TestModule::new(&crate::state::compile_flag_observer().unwrap());
    for operation in COMPARISONS {
        for prefix in [0xf2, 0xf3] {
            let code = code(operation, prefix, 1, false, false);
            let block = TestModule::new(&compile_block_from_bytes(0x1000, &code, 1).unwrap());
            for module in [TestModule::interpreter(), &block] {
                let mut machine = Machine::new(&code);
                machine.cpu.flags = record(0xfe);
                machine.cpu.flags.status_source.kind = 9;
                machine.cpu.instruction_count = 17;
                machine.cpu.registers.eax = 5;
                machine.cpu.registers.ecx = 4;
                machine.cpu.registers.esi = 0x4000;
                machine.cpu.registers.edi = 0x7ffe;
                machine.memory(0x4000, &[5; 4], ReadOnly);
                machine.memory(0x7ffe, &[if prefix == 0xf3 { 5 } else { 3 }; 2], ReadOnly);
                let initial = machine.state();
                let fault = machine.run(module, engine);
                let mut expected = initial;
                expected.cpu.registers.ecx = 2;
                expected.cpu.registers.edi = 0x8000;
                if operation == Operation::Cmps {
                    expected.cpu.registers.esi = 0x4002;
                }
                assert_eq!(fault.state, expected);
                assert_eq!(
                    fault.exit,
                    Exit::PageFault {
                        address: 0x8000,
                        error: 0
                    }
                );
                assert!(fault.dispatches.is_empty() && fault.machine_unchanged);

                machine.cpu = fault.state.cpu;
                // A restart from the original indices would now stop immediately.
                machine.memory(0x7ffe, &[if prefix == 0xf3 { 3 } else { 5 }; 2], ReadOnly);
                machine.memory(
                    0x8000,
                    if prefix == 0xf3 { &[5, 3] } else { &[3, 5] },
                    ReadOnly,
                );
                let before_resume = machine.state();
                let mut expected = before_resume.clone();
                expected.cpu.registers.ecx = 0;
                expected.cpu.registers.edi = 0x8002;
                if operation == Operation::Cmps {
                    expected.cpu.registers.esi = 0x4004;
                }
                expected.cpu.eip = 0x1002;
                expected.cpu.instruction_count = 18;
                let actual = machine.run(module, engine);
                let cpu_bytes = actual.state.cpu.to_bytes();
                let observed = engine.observe(&observer, &Input::new(&cpu_bytes), 1);
                let Event::Return {
                    outcome: Outcome::Returned(values),
                    snapshot,
                } = &observed.events[0]
                else {
                    panic!("flag observer did not return");
                };
                let bits = if prefix == 0xf3 {
                    [0, 0, 0, 0, 0, 0]
                } else {
                    [0, 1, 0, 1, 0, 0]
                };
                assert_eq!(values, &bits.map(Argument::I32));
                assert_eq!(snapshot.cpu, cpu_bytes);
                let before = before_resume.cpu.flags.bytes;
                let after = actual.state.cpu.flags.bytes;
                assert_eq!(
                    [
                        after.tf,
                        after.df,
                        after.nt,
                        after.ac,
                        after.id,
                        after.reserved
                    ],
                    [
                        before.tf,
                        before.df,
                        before.nt,
                        before.ac,
                        before.id,
                        before.reserved
                    ]
                );
                // The logical result is checked above; compare every other CPU and memory field.
                expected.cpu.flags = actual.state.cpu.flags;
                assert_eq!(actual.state, expected);
                assert_eq!(actual.exit, Exit::Dispatch(0x1002));
                assert_eq!(actual.dispatches, [(0x1002, expected)]);
                assert!(actual.machine_unchanged);
            }
        }
    }
}

#[test]
fn repaired_fault_resumes_with_remaining_comparisons() {
    repaired(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn repaired_fault_resumes_with_remaining_comparisons_v8() {
    repaired(Engine::V8);
}
