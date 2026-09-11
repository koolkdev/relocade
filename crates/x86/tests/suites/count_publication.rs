use crate::support::{
    cases::Permissions::ReadOnly,
    sequences::{test_sequences, Checkpoint, SequenceCase},
    step,
};

use step::{Argument, Event, Input, Observation, Outcome, Snapshot, TestModule};
use wasm86_x86::{compile_block_from_bytes, CpuState, Gpr32};
use wasmparser::{Operator, Parser, Payload, TypeRef, Validator};

// MOV EAX, [2000]; MOV ECX, [4000]; MOV EDX, [6000].
const READS: &[u8] = &[
    0xa1, 0x00, 0x20, 0x00, 0x00, 0x8b, 0x0d, 0x00, 0x40, 0x00, 0x00, 0x8b, 0x15, 0x00, 0x60, 0x00,
    0x00,
];

#[test]
fn only_exits_with_progress_read_and_write_instruction_count() {
    let module = compile_block_from_bytes(0x1000, READS, 3).unwrap();
    Validator::new().validate_all(&module.bytes).unwrap();
    let mut cpu_memory = None;
    let mut memory_index = 0;
    let mut function_index = 0;
    let mut entry = None;
    let mut exits = Vec::new();
    let mut loads = 0;
    let mut stores = 0;
    for payload in Parser::new(0).parse_all(&module.bytes) {
        match payload.unwrap() {
            Payload::ImportSection(imports) => {
                for import in imports {
                    let import = import.unwrap();
                    if matches!(import.ty, TypeRef::Func(_)) {
                        function_index += 1;
                    }
                    if matches!(import.ty, TypeRef::Memory(_)) {
                        if import.module == "wasm86" && import.name == "cpuState" {
                            cpu_memory = Some(memory_index);
                        }
                        memory_index += 1;
                    }
                }
            }
            Payload::ExportSection(exports) => {
                for export in exports {
                    let export = export.unwrap();
                    if export.name == module.entry {
                        entry = Some(export.index);
                    }
                }
            }
            Payload::CodeSectionEntry(body) if Some(function_index) == entry => {
                for operation in body.get_operators_reader().unwrap() {
                    match operation.unwrap() {
                        Operator::I32Load { memarg }
                            if Some(memarg.memory) == cpu_memory && memarg.offset == 144 =>
                        {
                            loads += 1;
                        }
                        Operator::I32Store { memarg }
                            if Some(memarg.memory) == cpu_memory && memarg.offset == 144 =>
                        {
                            stores += 1;
                        }
                        Operator::Return | Operator::ReturnCall { .. } => {
                            exits.push((loads, stores));
                            loads = 0;
                            stores = 0;
                        }
                        _ => {}
                    }
                }
                function_index += 1;
            }
            Payload::CodeSectionEntry(_) => function_index += 1,
            _ => {}
        }
    }
    // The first read's fault has no progress to publish. Two later faults and
    // the successful exit each publish their own completed instruction count.
    assert_eq!(exits, [(0, 0), (1, 1), (1, 1), (1, 1)]);
    assert_eq!((loads, stores), (0, 0));
}

fn completed_instruction_counts() -> Vec<SequenceCase> {
    let reads = [
        (
            &[0xa1, 0, 0x20, 0, 0][..],
            Gpr32::Eax,
            0x4433_2211_u32,
            0x2000,
            0x5000,
        ),
        (
            &[0x8b, 0x0d, 0, 0x40, 0, 0][..],
            Gpr32::Ecx,
            0x8877_6655,
            0x4000,
            0x7000,
        ),
        (
            &[0x8b, 0x15, 0, 0x60, 0, 0][..],
            Gpr32::Edx,
            0xccbb_aa99,
            0x6000,
            0x9000,
        ),
    ];
    let mut cases = Vec::new();
    for count in [37, 0xffff_fffe] {
        for readable in 0..=3 {
            let mut case = SequenceCase::preserving_flags(format!(
                "initial count {count}, readable pages {readable}"
            ))
            .instruction_count(count);
            for (index, &(code, register, value, address, frame)) in reads.iter().enumerate() {
                case = match index.cmp(&readable) {
                    std::cmp::Ordering::Less => case
                        .map_page(address >> 12, frame, ReadOnly)
                        .backing(frame, &value.to_le_bytes())
                        .step(Checkpoint::preserving_flags(code).register(register, value)),
                    std::cmp::Ordering::Equal => {
                        case.step(Checkpoint::preserving_flags(code).fault(address, 0))
                    }
                    std::cmp::Ordering::Greater => case.trailing_code(code, 1),
                };
            }
            cases.push(case);
        }
    }
    cases
}
test_sequences!(
    counts_at_each_success_and_fault_boundary,
    completed_instruction_counts()
);

#[test]
fn instruction_counts_reread_host_changes_between_invocations() {
    let module =
        compile_block_from_bytes(0x1000, &[0xb8, 7, 0, 0, 0, 0xb9, 9, 0, 0, 0], 2).unwrap();
    let mut initial = CpuState::filled(0xa5);
    initial.eip = 0x1000;
    initial.instruction_count = 37;
    let mut expected_cpu = initial;
    expected_cpu.registers.eax = 7;
    expected_cpu.registers.ecx = 9;
    expected_cpu.eip = 0x100a;
    let mut before_second = expected_cpu;
    before_second.instruction_count = 0xffff_fffe;
    let mut before_third = expected_cpu;
    before_third.instruction_count = 7;
    let input = Input {
        cpu_patches_before_calls: vec![
            vec![],
            vec![(0, before_second.to_bytes().to_vec())],
            vec![(0, before_third.to_bytes().to_vec())],
        ],
        ..Input::new(&initial.to_bytes())
    };
    let mut events = Vec::new();
    for count in [39_u32, 0, 9] {
        expected_cpu.instruction_count = count;
        let snapshot = Snapshot {
            cpu: expected_cpu.to_bytes().to_vec(),
            guest: None,
        };
        events.push(Event::Dispatch {
            eip: 4106,
            snapshot: snapshot.clone(),
        });
        events.push(Event::Return {
            outcome: Outcome::Returned(vec![Argument::I64(i64::MIN)]),
            snapshot,
        });
    }
    let expected = Observation {
        events,
        guest_unchanged: true,
        machine_unchanged: true,
    };
    assert_eq!(TestModule::new(&module).observe(&input, 3), expected);
}
