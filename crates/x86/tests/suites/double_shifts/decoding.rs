use wasm86_x86::{compile_block_from_bytes, BlockError};

use crate::support::{
    machine::{both, check, Exit, Step},
    step::TestModule,
};

use super::{expected, image, OPERATIONS};

#[test]
fn all_double_shift_forms_decode_their_source_address_and_count() {
    for operation in OPERATIONS {
        for code in [
            vec![0x0f, operation.opcode(false), 0xd0, 1],
            vec![0x0f, operation.opcode(true), 0xd0],
            vec![0x66, 0x0f, operation.opcode(false), 0xfc, 16],
            vec![0x66, 0x0f, operation.opcode(true), 0xd1],
            vec![0x0f, operation.opcode(false), 0x15, 0x20, 0x40, 0, 0, 32],
            vec![0x66, 0x0f, operation.opcode(true), 0x54, 0x8b, 0xfc],
        ] {
            for available in 0..code.len() {
                assert!(
                    matches!(
                        compile_block_from_bytes(0x1000, &code[..available], 1),
                        Err(BlockError::TruncatedInstruction { address: 0x1000, available: actual })
                            if actual == available
                    ),
                    "{operation:?} {code:02x?}, available {available}"
                );
            }
            let complete = compile_block_from_bytes(0x1000, &code, 1).unwrap();
            let with_suffix = [code, vec![0x0f]].concat();
            assert_eq!(
                compile_block_from_bytes(0x1000, &with_suffix, 1)
                    .unwrap()
                    .bytes,
                complete.bytes
            );
        }
    }
}

#[test]
fn cl_double_shifts_end_without_fetching_an_immediate() {
    for operation in OPERATIONS {
        for bits in [16, 32] {
            let mut code = if bits == 16 { vec![0x66] } else { vec![] };
            code.extend_from_slice(&[0x0f, operation.opcode(true), 0xd0]);
            let start = 0x2000 - code.len() as u32;
            let mut image = image(&[]);
            image.cpu.eip = start;
            image.cpu.registers.ecx = 0x8877_6601;
            image.data(0x3000 + (start & 0xfff), &code);
            let shifted = expected(
                operation,
                bits,
                image.cpu.registers.eax,
                image.cpu.registers.edx,
                1,
            );
            let mask = u32::MAX >> (32 - bits);
            let mut cpu = image.cpu;
            cpu.registers.eax = (cpu.registers.eax & !mask) | shifted.value;
            shifted.apply_flags(&mut cpu);
            cpu.eip = 0x2000;
            cpu.instruction_count = 0;
            both(
                TestModule::interpreter(),
                "CL double shift ends at the last mapped byte",
                &code,
                1,
                &image,
                &[Step {
                    cpu,
                    ram: &[],
                    exit: Exit::Dispatch(cpu.eip),
                }],
            );
        }
    }
}

#[test]
fn the_fifteenth_byte_can_complete_either_double_shift_count_form() {
    for operation in OPERATIONS {
        for from_cl in [false, true] {
            let mut code = vec![0x66; if from_cl { 12 } else { 11 }];
            code.extend_from_slice(&[0x0f, operation.opcode(from_cl), 0xd0]);
            if !from_cl {
                code.push(32);
            }
            let mut image = image(&[]);
            image.cpu.eip = 0x1ff1;
            image.cpu.registers.ecx = 0x8877_6620;
            image.data(0x3ff1, &code);
            let mut cpu = image.cpu;
            cpu.eip = 0x2000;
            cpu.instruction_count = 0;
            both(
                TestModule::interpreter(),
                "fifteen-byte double shift retains raw flags at masked zero",
                &code,
                1,
                &image,
                &[Step {
                    cpu,
                    ram: &[],
                    exit: Exit::Dispatch(cpu.eip),
                }],
            );
        }
    }
}

#[test]
fn missing_double_shift_fields_fault_before_operand_and_flag_effects() {
    for operation in OPERATIONS {
        for code in [
            vec![0x0f],
            vec![0x0f, operation.opcode(false)],
            vec![0x0f, operation.opcode(true), 0x14],
            vec![0x66, 0x0f, operation.opcode(true), 0x54, 0x8b],
            vec![0x0f, operation.opcode(false), 0x15, 0x20, 0x40, 0],
            vec![0x66, 0x0f, operation.opcode(false), 0x54, 0x8b, 0x80],
        ] {
            let start = 0x2000 - code.len() as u32;
            let mut image = image(&[]);
            image.cpu.eip = start;
            image.data(0x3000 + (start & 0xfff), &code);
            check(
                TestModule::interpreter(),
                "double shift fetches its encoding before accessing an operand",
                &image,
                &[Step {
                    cpu: image.cpu,
                    ram: &[],
                    exit: Exit::PageFault {
                        address: 0x2000,
                        error: 0x10,
                    },
                }],
            );
        }
    }
}

#[test]
fn length_limit_precedes_fetching_another_double_shift_field() {
    for operation in OPERATIONS {
        for (prefixes, suffix) in [
            (14, vec![0x0f]),
            (13, vec![0x0f, operation.opcode(false)]),
            (12, vec![0x0f, operation.opcode(false), 0xd0]),
            (12, vec![0x0f, operation.opcode(true), 0x14]),
            (11, vec![0x0f, operation.opcode(true), 0x54, 0x8b]),
        ] {
            let code = [vec![0x66; prefixes], suffix].concat();
            assert!(matches!(
                compile_block_from_bytes(0x1ff1, &code, 1),
                Err(BlockError::InstructionTooLong { address: 0x1ff1 })
            ));
            let mut image = image(&[]);
            image.cpu.eip = 0x1ff1;
            image.data(0x3ff1, &code);
            check(
                TestModule::interpreter(),
                "double shift would exceed fifteen bytes",
                &image,
                &[Step {
                    cpu: image.cpu,
                    ram: &[],
                    exit: Exit::Other(0x0002_0000_0000_0000),
                }],
            );
        }
    }
}
