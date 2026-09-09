use crate::support::machine;
use crate::support::step;

use machine::{Exit, Image, Step};
use step::{Argument, Event, Input, Observation, Outcome, Snapshot, TestModule};
use wasm86_x86::compile_block_from_bytes;
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

#[test]
fn instruction_counts_include_only_completed_instructions_at_each_fault() {
    let runtime = TestModule::interpreter();
    for counts in [[37, 38, 39, 40], [0xffff_fffe, 0xffff_ffff, 0, 1]] {
        for readable_pages in 0..=3 {
            let mut image = Image::new(READS);
            image.register(144, counts[0]);
            for &(page, frame, value) in [
                (2, 0x5000, 0x4433_2211_u32),
                (4, 0x7000, 0x8877_6655),
                (6, 0x9000, 0xccbb_aa99),
            ]
            .iter()
            .take(readable_pages)
            {
                image.map(page, frame, false);
                image.data(frame, &value.to_le_bytes());
            }
            let completed = [
                [(24, 0x4433_2211), (56, 0x1005), (144, counts[1])],
                [(28, 0x8877_6655), (56, 0x100b), (144, counts[2])],
                [(32, 0xccbb_aa99), (56, 0x1011), (144, counts[3])],
            ];
            let mut steps = completed
                .iter()
                .take(readable_pages)
                .map(|cpu| Step {
                    cpu,
                    ram: &[],
                    exit: Exit::Dispatch(cpu[1].1),
                })
                .collect::<Vec<_>>();
            let fault_cpu = [(144, counts[readable_pages])];
            if readable_pages < 3 {
                let address = [0x2000, 0x4000, 0x6000][readable_pages];
                steps.push(Step {
                    cpu: &fault_cpu,
                    ram: &[],
                    exit: Exit::PageFault { address, error: 0 },
                });
            }
            machine::both(
                runtime,
                &format!(
                    "initial count {}, readable pages {readable_pages}",
                    counts[0]
                ),
                READS,
                3,
                &image,
                &steps,
            );
        }
    }
}

#[test]
fn instruction_counts_reread_host_changes_between_invocations() {
    let module =
        compile_block_from_bytes(0x1000, &[0xb8, 7, 0, 0, 0, 0xb9, 9, 0, 0, 0], 2).unwrap();
    let mut initial = [0xa5; 152];
    initial[56..60].copy_from_slice(&0x1000_u32.to_le_bytes());
    initial[144..148].copy_from_slice(&37_u32.to_le_bytes());
    let input = Input {
        cpu_patches_before_calls: vec![
            vec![],
            vec![(144, vec![254, 255, 255, 255])],
            vec![(144, vec![7, 0, 0, 0])],
        ],
        ..Input::new(&initial)
    };
    let mut expected_cpu = initial;
    for (offset, value) in [(24, 7_u32), (28, 9), (56, 0x100a)] {
        expected_cpu[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    let mut events = Vec::new();
    for count in [39_u32, 0, 9] {
        expected_cpu[144..148].copy_from_slice(&count.to_le_bytes());
        let snapshot = Snapshot {
            cpu: expected_cpu.to_vec(),
            guest: None,
        };
        events.push(Event::Dispatch {
            eip: 4106,
            snapshot: snapshot.clone(),
        });
        events.push(Event::Return {
            outcome: Outcome::Returned(Some(Argument::I64(i64::MIN))),
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

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn instruction_counts_execute_in_optimizing_v8() {
    let mut image = Image::new(READS);
    image.register(144, 0xffff_fffe);
    image.map(2, 0x5000, false);
    image.map(4, 0x7000, false);
    image.data(0x5000, &0x4433_2211_u32.to_le_bytes());
    image.data(0x7000, &0x8877_6655_u32.to_le_bytes());
    let first = [(24, 0x4433_2211), (56, 0x1005), (144, 0xffff_ffff)];
    let second = [(28, 0x8877_6655), (56, 0x100b), (144, 0)];
    let steps = [
        Step {
            cpu: &first,
            ram: &[],
            exit: Exit::Dispatch(0x1005),
        },
        Step {
            cpu: &second,
            ram: &[],
            exit: Exit::Dispatch(0x100b),
        },
        Step {
            cpu: &[],
            ram: &[],
            exit: Exit::PageFault {
                address: 0x00006000,
                error: 0x0,
            },
        },
    ];
    assert_eq!(
        TestModule::interpreter().observe_v8(&image.input(), 3),
        machine::expected(&image, &steps),
    );
    let block = TestModule::new(&compile_block_from_bytes(0x1000, READS, 3).unwrap());
    let cpu = [first, second].concat();
    assert_eq!(
        block.observe_v8(&image.input(), 1),
        machine::expected(
            &image,
            &[Step {
                cpu: &cpu,
                ram: &[],
                exit: Exit::PageFault {
                    address: 0x00006000,
                    error: 0x0
                }
            }]
        ),
    );
}
