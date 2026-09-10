use wasm86_x86::{compile_block_from_bytes, BlockError};

use crate::support::{
    machine::{both, check, Exit, Step},
    step::TestModule,
};

use super::{expected, image, prior_flags, OPERATIONS};

#[test]
fn carry_rotate_forms_decode_their_complete_address_and_count() {
    for operation in OPERATIONS {
        let group = operation.extension() << 3;
        for code in [
            vec![0xd0, 0xc4 | group],
            vec![0xd1, 0x05 | group, 0x20, 0x40, 0, 0],
            vec![0xd2, 0xc4 | group],
            vec![0x66, 0xd3, 0xc1 | group],
            vec![0xc0, 0xc5 | group, 3],
            vec![0x66, 0xc1, 0x44 | group, 0x8b, 0xfc, 32],
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
fn implicit_and_cl_carry_rotates_do_not_fetch_an_immediate() {
    for operation in OPERATIONS {
        for (opcode, bits, count) in [(0xd0, 8, 1), (0xd1, 32, 1), (0xd2, 8, 32), (0xd3, 32, 32)] {
            let code = [opcode, 0xc0 | (operation.extension() << 3)];
            let mut image = image(&[], 1);
            image.cpu.eip = 0x1ffe;
            image.cpu.registers.ecx = 0x8877_6620;
            image.data(0x3ffe, &code);
            let result = expected(
                operation,
                bits,
                image.cpu.registers.eax,
                count,
                prior_flags(1),
            );
            let mut cpu = image.cpu;
            cpu.registers.eax = if bits == 8 {
                0x4433_2200 | result.value
            } else {
                result.value
            };
            result.apply_flags(&mut cpu);
            cpu.eip = 0x2000;
            cpu.instruction_count = 0;
            both(
                TestModule::interpreter(),
                "carry rotate ends at the last mapped byte",
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
fn the_fifteenth_byte_can_supply_a_masked_zero_carry_rotate_count() {
    for operation in OPERATIONS {
        let code = [
            vec![0x66; 12],
            vec![0xc1, 0xc0 | (operation.extension() << 3), 32],
        ]
        .concat();
        let mut image = image(&[], 1);
        image.cpu.eip = 0x1ff1;
        image.data(0x3ff1, &code);
        let mut cpu = image.cpu;
        cpu.eip = 0x2000;
        cpu.instruction_count = 0;
        both(
            TestModule::interpreter(),
            "fifteen-byte carry rotate retains raw flags",
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

#[test]
fn missing_encoding_fields_fault_before_operand_and_flag_effects() {
    for operation in OPERATIONS {
        let group = operation.extension() << 3;
        for code in [
            vec![0xd0],
            vec![0xd2, 0x44 | group],
            vec![0x66, 0xd3, 0x44 | group, 0x8b],
            vec![0xc0, 0xc4 | group],
            vec![0xc1, 0x05 | group, 0x20, 0x40, 0],
            vec![0x66, 0xc1, 0x44 | group, 0x8b, 0x80],
        ] {
            let start = 0x2000 - code.len() as u32;
            let mut image = image(&[], 1);
            image.cpu.eip = start;
            image.data(0x3000 + (start & 0xfff), &code);
            check(
                TestModule::interpreter(),
                "carry rotate requires its encoding before any effect",
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
fn length_limit_precedes_fetching_another_carry_rotate_field() {
    for operation in OPERATIONS {
        let group = operation.extension() << 3;
        for (prefixes, suffix) in [
            (14, vec![0xd0]),
            (13, vec![0xc1, 0xc0 | group]),
            (13, vec![0xd2, 0x04 | group]),
            (12, vec![0xd3, 0x44 | group, 0x8b]),
        ] {
            let code = [vec![0x66; prefixes], suffix].concat();
            assert!(matches!(
                compile_block_from_bytes(0x1ff1, &code, 1),
                Err(BlockError::InstructionTooLong { address: 0x1ff1 })
            ));
            let mut image = image(&[], 1);
            image.cpu.eip = 0x1ff1;
            image.data(0x3ff1, &code);
            check(
                TestModule::interpreter(),
                "carry rotate would exceed fifteen bytes",
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
