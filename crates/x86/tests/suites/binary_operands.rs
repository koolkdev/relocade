use wasm86_x86::compile_block_from_bytes;
use wasmparser::{Operator, Parser, Payload, TypeRef, Validator};

use crate::support::arithmetic;
use crate::support::machine;
use crate::support::step;
use arithmetic::image;
use machine::{both, Exit, Step};
use step::TestModule;
#[path = "binary_operands/faults.rs"]
mod faults;
#[path = "binary_operands/memory.rs"]
mod memory;

#[test]
fn test_and_compare_only_read_guest_memory_while_updates_store() {
    for (code, writes) in [
        (&[0x38, 0x03][..], false),
        (&[0x66, 0x39, 0x03][..], false),
        (&[0x39, 0x03][..], false),
        (&[0x00, 0x03][..], true),
        (&[0x66, 0x01, 0x03][..], true),
        (&[0x01, 0x03][..], true),
        (&[0x84, 0x03][..], false),
        (&[0x66, 0x85, 0x03][..], false),
        (&[0x85, 0x03][..], false),
        (&[0xf6, 0x03, 0x80][..], false),
        (&[0x66, 0xf7, 0x03, 0x00, 0x80][..], false),
        (&[0xf7, 0x03, 0x00, 0x00, 0x00, 0x80][..], false),
        (&[0x28, 0x03][..], true),
        (&[0x66, 0x29, 0x03][..], true),
        (&[0x29, 0x03][..], true),
        (&[0x20, 0x03][..], true),
        (&[0x66, 0x21, 0x03][..], true),
        (&[0x21, 0x03][..], true),
        (&[0x08, 0x03][..], true),
        (&[0x66, 0x09, 0x03][..], true),
        (&[0x09, 0x03][..], true),
        (&[0x30, 0x03][..], true),
        (&[0x66, 0x31, 0x03][..], true),
        (&[0x31, 0x03][..], true),
    ] {
        let module = compile_block_from_bytes(0x1000, code, 1).unwrap();
        Validator::new().validate_all(&module.bytes).unwrap();
        let mut guest = None;
        let mut memory_index = 0;
        let mut loads = 0;
        let mut stores = 0;
        for payload in Parser::new(0).parse_all(&module.bytes) {
            match payload.unwrap() {
                Payload::ImportSection(section) => {
                    for import in section {
                        let import = import.unwrap();
                        if matches!(import.ty, TypeRef::Memory(_)) {
                            if import.name == "guest" {
                                guest = Some(memory_index);
                            }
                            memory_index += 1;
                        }
                    }
                }
                Payload::CodeSectionEntry(body) => {
                    for operator in body.get_operators_reader().unwrap() {
                        match operator.unwrap() {
                            Operator::I32Load { memarg }
                            | Operator::I32Load8U { memarg }
                            | Operator::I32Load16U { memarg }
                                if Some(memarg.memory) == guest =>
                            {
                                loads += 1
                            }
                            Operator::I32Store { memarg }
                            | Operator::I32Store8 { memarg }
                            | Operator::I32Store16 { memarg }
                                if Some(memarg.memory) == guest =>
                            {
                                stores += 1
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
        assert!(guest.is_some() && loads > 0, "{code:02x?}");
        assert_eq!(stores > 0, writes, "{code:02x?}");
    }
}

#[test]
fn register_aliases() {
    let step = TestModule::interpreter();
    for (name, code, eax, result, left, right, kind) in [
        (
            "AL reads old AH",
            &[0x00, 0xe0][..],
            0x4433_7f81,
            0x4433_7f00,
            0x81,
            0x7f,
            2,
        ),
        (
            "AH reads old AL",
            &[0x00, 0xc4][..],
            0x4433_8080,
            0x4433_0080,
            0x80,
            0x80,
            2,
        ),
        (
            "dword self addition uses the old value",
            &[0x01, 0xc0][..],
            0x8000_0000,
            0,
            0x8000_0000,
            0x8000_0000,
            10,
        ),
        (
            "AH compare leaves both aliases intact",
            &[0x38, 0xc4][..],
            0x4433_7efe,
            0x4433_7efe,
            0x7e,
            0xfe,
            1,
        ),
        (
            "accumulator byte ADD ignores operand prefix",
            &[0x66, 0x04, 1][..],
            0x4433_22ff,
            0x4433_2200,
            0xff,
            1,
            2,
        ),
        (
            "accumulator word CMP reads two immediate bytes",
            &[0x66, 0x3d, 0x11, 0x22][..],
            0x4433_2211,
            0x4433_2211,
            0x2211,
            0x2211,
            5,
        ),
    ] {
        let mut image = image(code);
        image.cpu.registers.eax = eax;
        let next = 0x1000 + code.len() as u32;
        let mut expected = image.cpu;
        expected.flags.kind = kind;
        expected.flags.left = left;
        expected.flags.right = right;
        expected.registers.eax = result;
        expected.eip = next;
        expected.instruction_count = 0;
        both(
            step,
            name,
            code,
            1,
            &image,
            &[Step {
                cpu: expected,
                ram: &[],
                exit: Exit::Dispatch(next),
            }],
        );
    }
    let code = [
        0x04, 1, 0x66, 0x0f, 0x94, 0xfc, 0xb0, 0x7f, 0x0f, 0x92, 0xc0,
    ];
    let mut image = image(&code);
    image.cpu.registers.eax = 0x4433_22ff;
    let mut expected_cpu = image.cpu;
    let mut steps = Vec::new();

    expected_cpu.flags.kind = 2;
    expected_cpu.flags.left = 0xff;
    expected_cpu.flags.right = 1;
    expected_cpu.registers.eax = 0x4433_2200;
    expected_cpu.eip = 0x1002;
    expected_cpu.instruction_count = 0;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[],
        exit: Exit::Dispatch(0x1002),
    });

    expected_cpu.registers.eax = 0x4433_0100;
    expected_cpu.eip = 0x1006;
    expected_cpu.instruction_count = 1;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[],
        exit: Exit::Dispatch(0x1006),
    });

    expected_cpu.registers.eax = 0x4433_017f;
    expected_cpu.eip = 0x1008;
    expected_cpu.instruction_count = 2;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[],
        exit: Exit::Dispatch(0x1008),
    });

    expected_cpu.registers.eax = 0x4433_0101;
    expected_cpu.eip = 0x100b;
    expected_cpu.instruction_count = 3;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[],
        exit: Exit::Dispatch(0x100b),
    });

    both(
        step,
        "SETcc and MOV preserve prior ADD flags while changing aliases",
        &code,
        4,
        &image,
        &steps,
    );
}
