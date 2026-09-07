use super::{AccessFault, Intent, Memory};
use wasm86_compiler::{
    AtLeast, BuildError, FunctionBuilder, MemoryInt, Program, Signature, Type, I16, I32, I64, I8,
};
use wasmparser::{Operator, Parser, Payload, TypeRef, Validator};

use crate::test_step::ModuleFile;

fn return_fault(body: &mut FunctionBuilder<'_>, fault: &AccessFault) -> Result<(), BuildError> {
    body.if_(&fault.condition, |arm| {
        arm.return_(crate::state::exit::page_fault(&fault.address, &fault.error))
    })
}

fn define_width<T: MemoryInt>(program: &mut Program, memory: Memory, name: &str)
where
    I64: AtLeast<T>,
{
    let read = program
        .function(
            Signature {
                parameters: vec![Type::I32],
                result: Type::I64,
            },
            |mut body| {
                let address = body.parameter::<I32>(0)?;
                let access = memory.resolve_access::<T>(&mut body, &address, Intent::Read)?;
                return_fault(&mut body, &access.fault)?;
                let value = memory.read(&mut body, &access)?;
                body.return_(value.unsigned().extend::<I64>())
            },
        )
        .unwrap();
    program.export(&format!("read{name}"), read).unwrap();

    let write = program
        .function(
            Signature {
                parameters: vec![Type::I32, T::TYPE],
                result: Type::I64,
            },
            |mut body| {
                let address = body.parameter::<I32>(0)?;
                let value = body.parameter::<T>(1)?;
                let access = memory.resolve_access::<T>(&mut body, &address, Intent::Write)?;
                return_fault(&mut body, &access.fault)?;
                memory.write(&mut body, &access, &value)?;
                body.return_(7)
            },
        )
        .unwrap();
    program.export(&format!("write{name}"), write).unwrap();
}

fn accesses() -> Vec<u8> {
    let mut program = Program::new();
    let memory = Memory::declare(&mut program).unwrap();
    define_width::<I8>(&mut program, memory, "8");
    define_width::<I16>(&mut program, memory, "16");
    define_width::<I32>(&mut program, memory, "32");
    define_width::<I64>(&mut program, memory, "64");
    program.compile().unwrap()
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
    argument: &'static str,
    result: &'static str,
    missing_read: u64,
}

const WIDTHS: &[WidthCase] = &[
    WidthCase {
        name: "8",
        linear: 0x4fff,
        physical: 0x8fff,
        first: &[0x91],
        second: &[],
        argument: "[\"i32\",145]",
        result: "145",
        missing_read: 0x0004_0000_0000_4fff,
    },
    WidthCase {
        name: "16",
        linear: 0x4fff,
        physical: 0x8fff,
        first: &[0xfe],
        second: &[0x91],
        argument: "[\"i32\",37374]",
        result: "37374",
        missing_read: 0x0004_0000_0000_4fff,
    },
    WidthCase {
        name: "32",
        linear: 0x4ffe,
        physical: 0x8ffe,
        first: &[0x78, 0x56],
        second: &[0x34, 0x92],
        argument: "[\"i32\",2452903544]",
        result: "2452903544",
        missing_read: 0x0004_0000_0000_4ffe,
    },
    WidthCase {
        name: "64",
        linear: 0x4ffc,
        physical: 0x8ffc,
        first: &[0x11, 0x22, 0x33, 0x44],
        second: &[0x55, 0x66, 0x77, 0x88],
        argument: "[\"i64\",\"-8613303245920329199\"]",
        result: "-8613303245920329199",
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
    arguments: &str,
) -> String {
    let first_bytes = [&[0xa5][..], first, &[0x5a]].concat();
    let second_bytes = [second, &[0x5a]].concat();
    format!(
        "[{:?},[[{}, {:?}],[{}, {:?}]],[[16,[{},128,0,0]],[20,[{},{},0,0]]],{},true]",
        [0xa5u8; 152],
        case.physical - 1,
        first_bytes,
        second_frame,
        second_bytes,
        first_permissions,
        second_permissions,
        second_frame >> 8,
        arguments
    )
}

fn check(module: &ModuleFile, flags: &[&str], input: &str, result: &str, changes: &[(u32, u8)]) {
    let changes = changes
        .iter()
        .map(|(offset, value)| format!("[{offset},{value}]"))
        .collect::<Vec<_>>()
        .join(",");
    let expected = format!(
        "return {result}\nstate {}\nguest at return [{changes}]\nguest {}\nmachine unchanged\n",
        "a5".repeat(152),
        if changes.is_empty() {
            "unchanged"
        } else {
            "changed"
        }
    );
    assert_eq!(
        module.observe(flags, input, 1),
        expected,
        "{}, flags {flags:?}",
        module.entry
    );
}

fn check_execution(flags: &[&str]) {
    let bytes = accesses();
    for case in WIDTHS {
        let read = ModuleFile::new(&crate::CompiledModule {
            bytes: bytes.clone(),
            entry: format!("read{}", case.name),
        });
        let write = ModuleFile::new(&crate::CompiledModule {
            bytes: bytes.clone(),
            entry: format!("write{}", case.name),
        });
        let read_args = format!("[[\"i32\",{}]]", case.linear);
        let write_args = format!("[[\"i32\",{}],{}]", case.linear, case.argument);
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
            check(&read, flags, &input, case.result, &[]);
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
            check(&write, flags, &input, "7", &changes);
        }
        let input = image(case, 0xa000, 0, 1, case.first, case.second, &read_args);
        check(&read, flags, &input, &case.missing_read.to_string(), &[]);

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
        check(&write, flags, &input, &fault.to_string(), &[]);
    }
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn memory_widths_execute_in_v8() {
    check_execution(&[]);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn memory_widths_execute_in_optimizing_v8() {
    check_execution(&[
        "--no-liftoff",
        "--no-wasm-lazy-compilation",
        "--no-wasm-tier-up",
    ]);
}
