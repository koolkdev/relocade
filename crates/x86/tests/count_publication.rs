#[path = "support/machine.rs"]
#[allow(dead_code)]
mod machine;
#[path = "support/step.rs"]
mod step;

use std::fmt::Write as _;

use machine::{Exit, Image, Step};
use step::ModuleFile;
use wasm86_x86::{compile_block_from_bytes, compile_interpreter_step, CompiledModule};
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

fn check_fault_counts(flags: &[&str]) {
    let runtime = ModuleFile::new(&compile_interpreter_step().unwrap());
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
                    exit: Exit::Fault((4 << 48) | address),
                });
            }
            machine::both(
                &runtime,
                flags,
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

fn check_host_changes_between_invocations(flags: &[&str]) {
    let module =
        compile_block_from_bytes(0x1000, &[0xb8, 7, 0, 0, 0, 0xb9, 9, 0, 0, 0], 2).unwrap();
    let mut initial = [0xa5; 152];
    initial[56..60].copy_from_slice(&0x1000_u32.to_le_bytes());
    initial[144..148].copy_from_slice(&37_u32.to_le_bytes());
    let patches = "[[],[[144,[254,255,255,255]]],[[144,[7,0,0,0]]]]";
    let input = format!("[{initial:?},[],[],[],false,{patches}]");
    let mut expected_cpu = initial;
    for (offset, value) in [(24, 7_u32), (28, 9), (56, 0x100a)] {
        expected_cpu[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    let mut expected = String::new();
    for count in [39_u32, 0, 9] {
        expected_cpu[144..148].copy_from_slice(&count.to_le_bytes());
        let mut state = String::new();
        for byte in expected_cpu {
            write!(&mut state, "{byte:02x}").unwrap();
        }
        writeln!(&mut expected, "dispatch(4106) {state}").unwrap();
        writeln!(&mut expected, "return -9223372036854775808\nstate {state}").unwrap();
    }
    expected.push_str("guest unchanged\nmachine unchanged\n");
    assert_eq!(ModuleFile::new(&module).observe(flags, &input, 3), expected);
}

fn check_execution(flags: &[&str]) {
    check_fault_counts(flags);
    check_host_changes_between_invocations(flags);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn instruction_counts_execute_in_v8() {
    check_execution(&[]);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn instruction_counts_execute_in_optimizing_v8() {
    check_execution(&[
        "--no-liftoff",
        "--no-wasm-lazy-compilation",
        "--no-wasm-tier-up",
    ]);
}
