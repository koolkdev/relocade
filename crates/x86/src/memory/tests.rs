use super::{AccessFault, Intent, Memory};
use wasm86_compiler::{
    AtLeast, BuildError, FunctionBuilder, MemoryInt, Program, Signature, Type, I16, I32, I64, I8,
};
use wasmparser::{Operator, Parser, Payload, TypeRef, Validator};

use crate::test_step::{Argument, Event, Input, Observation, Outcome, Snapshot, TestModule};
use crate::CpuState;

mod helpers;
mod required_reads;

fn return_fault(body: FunctionBuilder<'_>, fault: AccessFault) -> Result<(), BuildError> {
    body.return_(crate::state::exit::page_fault(&fault.address, &fault.error))
}

fn define_read<T: MemoryInt>(program: &mut Program, memory: &Memory, name: &str)
where
    I64: AtLeast<T>,
{
    let read = program
        .function(
            Signature {
                parameters: vec![Type::I32],
                result: Some(Type::I64),
            },
            |mut body| {
                let address = body.parameter::<I32>(0)?;
                let access =
                    memory.resolve_access::<T>(&mut body, &address, Intent::Read, return_fault)?;
                let value = memory.read(&mut body, &access)?;
                body.return_(value.unsigned().extend::<I64>())
            },
        )
        .unwrap();
    program.export(name, read).unwrap();
}

fn define_write<T: MemoryInt>(program: &mut Program, memory: &Memory, name: &str) {
    let write = program
        .function(
            Signature {
                parameters: vec![Type::I32, T::TYPE],
                result: Some(Type::I64),
            },
            |mut body| {
                let address = body.parameter::<I32>(0)?;
                let value = body.parameter::<T>(1)?;
                let access =
                    memory.resolve_access::<T>(&mut body, &address, Intent::Write, return_fault)?;
                memory.write(&mut body, &access, &value)?;
                body.return_(7)
            },
        )
        .unwrap();
    program.export(name, write).unwrap();
}

fn accesses() -> Vec<u8> {
    let mut program = Program::new();
    let memory = Memory::declare(&mut program).unwrap();
    define_read::<I8>(&mut program, &memory, "read8");
    define_write::<I8>(&mut program, &memory, "write8");
    define_read::<I16>(&mut program, &memory, "read16");
    define_write::<I16>(&mut program, &memory, "write16");
    define_read::<I32>(&mut program, &memory, "read32");
    define_write::<I32>(&mut program, &memory, "write32");
    define_read::<I64>(&mut program, &memory, "read64");
    define_write::<I64>(&mut program, &memory, "write64");
    program.compile().unwrap()
}

#[test]
fn access_fault_handlers_must_terminate_the_denied_path() {
    fn reject_fallthrough<T: MemoryInt>() {
        let mut program = Program::new();
        let memory = Memory::declare(&mut program).unwrap();
        let result = program.function(
            Signature {
                parameters: vec![Type::I32],
                result: Some(Type::I64),
            },
            |mut body| {
                let address = body.parameter::<I32>(0)?;
                memory.resolve_access::<T>(
                    &mut body,
                    &address,
                    Intent::Write,
                    |_fault_body, _fault| Ok(()),
                )?;
                body.return_(7)
            },
        );
        assert_eq!(result.err(), Some(BuildError::IncompleteBranch));
    }

    reject_fallthrough::<I8>();
    reject_fallthrough::<I32>();
}

#[test]
fn contiguous_accesses_keep_native_loads_and_stores_for_every_storage_width() {
    let bytes = accesses();
    Validator::new().validate_all(&bytes).unwrap();
    let mut operations = std::collections::BTreeSet::new();
    let mut guest_index = None;
    let mut memories = 0;
    for payload in Parser::new(0).parse_all(&bytes) {
        let payload = payload.unwrap();
        if let Payload::ImportSection(imports) = &payload {
            for import in imports.clone() {
                let import = import.unwrap();
                if matches!(import.ty, TypeRef::Memory(_)) {
                    if import.module == "wasm86" && import.name == "guest" {
                        guest_index = Some(memories);
                    }
                    memories += 1;
                }
            }
        }
        if let Payload::CodeSectionEntry(body) = payload {
            let guest_index = guest_index.expect("guest memory is imported");
            for operator in body.get_operators_reader().unwrap() {
                let name = match operator.unwrap() {
                    Operator::I32Load8U { memarg } if memarg.memory == guest_index => "load8",
                    Operator::I32Load16U { memarg } if memarg.memory == guest_index => "load16",
                    Operator::I32Load { memarg } if memarg.memory == guest_index => "load32",
                    Operator::I64Load { memarg } if memarg.memory == guest_index => "load64",
                    Operator::I32Store8 { memarg } if memarg.memory == guest_index => "store8",
                    Operator::I32Store16 { memarg } if memarg.memory == guest_index => "store16",
                    Operator::I32Store { memarg } if memarg.memory == guest_index => "store32",
                    Operator::I64Store { memarg } if memarg.memory == guest_index => "store64",
                    _ => continue,
                };
                operations.insert(name);
            }
        }
    }
    assert_eq!(
        operations.into_iter().collect::<Vec<_>>(),
        ["load16", "load32", "load64", "load8", "store16", "store32", "store64", "store8"]
    );
}

struct WidthCase {
    name: &'static str,
    linear: u32,
    physical: u32,
    first: &'static [u8],
    second: &'static [u8],
    argument: Argument,
    result: i64,
    missing_read: u64,
}

const WIDTHS: &[WidthCase] = &[
    WidthCase {
        name: "8",
        linear: 0x4fff,
        physical: 0x8fff,
        first: &[0x91],
        second: &[],
        argument: Argument::I32(145),
        result: 145,
        missing_read: 0x0004_0000_0000_4fff,
    },
    WidthCase {
        name: "16",
        linear: 0x4fff,
        physical: 0x8fff,
        first: &[0xfe],
        second: &[0x91],
        argument: Argument::I32(37374),
        result: 37374,
        missing_read: 0x0004_0000_0000_4fff,
    },
    WidthCase {
        name: "32",
        linear: 0x4ffe,
        physical: 0x8ffe,
        first: &[0x78, 0x56],
        second: &[0x34, 0x92],
        argument: Argument::I32(2452903544u32 as i32),
        result: 2452903544,
        missing_read: 0x0004_0000_0000_4ffe,
    },
    WidthCase {
        name: "64",
        linear: 0x4ffc,
        physical: 0x8ffc,
        first: &[0x11, 0x22, 0x33, 0x44],
        second: &[0x55, 0x66, 0x77, 0x88],
        argument: Argument::I64(-8613303245920329199),
        result: -8613303245920329199,
        missing_read: 0x0004_0000_0000_4ffc,
    },
];

fn image(
    case: &WidthCase,
    second_frame: u32,
    first_permissions: u8,
    second_permissions: u8,
    first: &[u8],
    second: &[u8],
    arguments: &[Argument],
) -> Input {
    let first_bytes = [&[0xa5][..], first, &[0x5a]].concat();
    let second_bytes = [second, &[0x5a]].concat();
    Input {
        guest: vec![
            (case.physical - 1, first_bytes),
            (second_frame, second_bytes),
        ],
        machine: vec![
            (
                16,
                (0x8000 | u32::from(first_permissions))
                    .to_le_bytes()
                    .to_vec(),
            ),
            (
                20,
                (second_frame | u32::from(second_permissions))
                    .to_le_bytes()
                    .to_vec(),
            ),
        ],
        arguments: arguments.to_vec(),
        observe_guest: true,
        ..Input::new(&CpuState::filled(0xa5).to_bytes())
    }
}

fn check(module: &TestModule, input: &Input, result: i64, changes: &[(u32, u8)]) {
    assert_eq!(
        module.observe(input, 1),
        Observation {
            events: vec![Event::Return {
                outcome: Outcome::Returned(Some(Argument::I64(result))),
                snapshot: Snapshot {
                    cpu: CpuState::filled(0xa5).to_bytes().to_vec(),
                    guest: Some(changes.to_vec()),
                },
            }],
            guest_unchanged: changes.is_empty(),
            machine_unchanged: true,
        },
        "{}",
        module.entry
    );
}

#[test]
fn memory_widths_execute_in_wasmtime() {
    let bytes = accesses();
    for case in WIDTHS {
        let read = TestModule::new(&crate::CompiledModule {
            bytes: bytes.clone(),
            entry: format!("read{}", case.name),
        });
        let write = TestModule::new(&crate::CompiledModule {
            bytes: bytes.clone(),
            entry: format!("write{}", case.name),
        });
        let read_args = [Argument::I32(case.linear as i32)];
        let write_args = [Argument::I32(case.linear as i32), case.argument];
        let initial_first = vec![0xff; case.first.len()];
        let initial_second = vec![0xff; case.second.len()];
        for second_frame in [0x9000, 0xa000] {
            // Unrelated PTE bits must not change permissions or frame selection.
            let input = image(
                case,
                second_frame,
                0x0d,
                0x21,
                case.first,
                case.second,
                &read_args,
            );
            check(&read, &input, case.result, &[]);
            let input = image(
                case,
                second_frame,
                3,
                3,
                &initial_first,
                &initial_second,
                &write_args,
            );
            let changes = case
                .first
                .iter()
                .enumerate()
                .map(|(i, byte)| (case.physical + i as u32, *byte))
                .chain(
                    case.second
                        .iter()
                        .enumerate()
                        .map(|(i, byte)| (second_frame + i as u32, *byte)),
                )
                .collect::<Vec<_>>();
            check(&write, &input, 7, &changes);
        }
        let input = image(case, 0xa000, 0, 1, case.first, case.second, &read_args);
        check(&read, &input, case.missing_read as i64, &[]);

        // A one-byte access ends on the first page; wider cases need the second page too.
        let (first_permissions, fault) = if case.second.is_empty() {
            (0x0d, 0x0004_0003_0000_4fffu64)
        } else {
            (3, 0x0004_0003_0000_5000u64)
        };
        let input = image(
            case,
            0xa000,
            first_permissions,
            1,
            &initial_first,
            &initial_second,
            &write_args,
        );
        check(&write, &input, fault as i64, &[]);
    }
}

#[test]
fn aligned_and_unaligned_single_page_transfers_ignore_unrelated_pte_bits() {
    let bytes = accesses();
    for case in WIDTHS.iter().filter(|case| !case.second.is_empty()) {
        let read = TestModule::new(&crate::CompiledModule {
            bytes: bytes.clone(),
            entry: format!("read{}", case.name),
        });
        let write = TestModule::new(&crate::CompiledModule {
            bytes: bytes.clone(),
            entry: format!("write{}", case.name),
        });
        let payload = [case.first, case.second].concat();
        for offset in [0, 1] {
            let address = 0x4000 + offset;
            let physical = 0x8000 + offset;
            let mut input = Input {
                guest: vec![(physical - 1, [&[0xa5][..], &payload, &[0x5a]].concat())],
                // All unrelated low bits are set; the next virtual page is absent.
                machine: vec![(16, 0x8fffu32.to_le_bytes().to_vec())],
                arguments: vec![Argument::I32(address as i32)],
                observe_guest: true,
                ..Input::new(&CpuState::filled(0xa5).to_bytes())
            };
            check(&read, &input, case.result, &[]);

            input.guest[0].1[1..1 + payload.len()].fill(0xff);
            input.arguments.push(case.argument);
            let changes = payload
                .iter()
                .enumerate()
                .map(|(index, byte)| (physical + index as u32, *byte))
                .collect::<Vec<_>>();
            check(&write, &input, 7, &changes);
        }
    }
}

#[test]
fn wrapping_writes_report_range_faults_before_read_only_page_faults() {
    let bytes = accesses();
    for (entry, address, value, fault) in [
        (
            "write16",
            0xffff_ffffu32,
            Argument::I32(0x1234),
            0x0004_0002_ffff_ffffi64,
        ),
        (
            "write32",
            0xffff_fffe,
            Argument::I32(0x1234_5678),
            0x0004_0002_ffff_fffe,
        ),
        (
            "write64",
            0xffff_fffc,
            Argument::I64(0x0102_0304_0506_0708),
            0x0004_0002_ffff_fffc,
        ),
    ] {
        let write = TestModule::new(&crate::CompiledModule {
            bytes: bytes.clone(),
            entry: entry.into(),
        });
        let input = Input {
            guest: vec![(0x8ff8, vec![0xa5; 8]), (0xa000, vec![0x5a; 8])],
            // The final page is present and read-only, page zero is writable,
            // and unrelated PTE bits cannot override the range-wrap fault.
            machine: vec![
                (0x003f_fffc, 0x8ffdu32.to_le_bytes().to_vec()),
                (0, 0xafffu32.to_le_bytes().to_vec()),
            ],
            arguments: vec![Argument::I32(address as i32), value],
            observe_guest: true,
            ..Input::new(&CpuState::filled(0xa5).to_bytes())
        };
        check(&write, &input, fault, &[]);
    }
}
