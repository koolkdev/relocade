use wasm86_x86::{compile_block_from_bytes, BlockError};

use crate::support::{
    machine::{both, check, Exit, Step},
    step::TestModule,
};

use super::{expected, image, prior_flags, OPERATIONS};

#[test]
fn register_and_immediate_bit_forms_decode_the_complete_address_and_index() {
    for operation in OPERATIONS {
        let group = operation.extension() << 3;
        for code in [
            vec![0x0f, operation.register_opcode(), 0xd0],
            vec![0x66, 0x0f, operation.register_opcode(), 0xe4],
            vec![0x0f, 0xba, 0xc0 | group, 255],
            vec![0x66, 0x0f, 0xba, 0x03 | group, 16],
            vec![0x0f, operation.register_opcode(), 0x15, 0x20, 0x40, 0, 0],
            vec![0x66, 0x0f, operation.register_opcode(), 0x54, 0x8b, 0xfc],
            vec![0x66, 0x0f, 0xba, 0x44 | group, 0x8b, 0xfc, 255],
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
fn register_index_forms_end_without_fetching_an_immediate() {
    for operation in OPERATIONS {
        for bits in [16, 32] {
            let mut code = if bits == 16 { vec![0x66] } else { vec![] };
            code.extend_from_slice(&[0x0f, operation.register_opcode(), 0xd0]);
            let start = 0x2000 - code.len() as u32;
            let mut image = image(&[]);
            image.cpu.eip = start;
            image.cpu.registers.edx = u32::MAX;
            image.data(0x3000 + (start & 0xfff), &code);
            let result = expected(operation, bits, image.cpu.registers.eax, u32::MAX);
            let mask = u32::MAX >> (32 - bits);
            let mut cpu = image.cpu;
            cpu.registers.eax = (cpu.registers.eax & !mask) | result.value;
            result.apply_flags(&mut cpu, prior_flags());
            cpu.eip = 0x2000;
            cpu.instruction_count = 0;
            both(
                TestModule::interpreter(),
                "register bit index ends at the last mapped byte",
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
fn the_fifteenth_byte_can_complete_either_bit_index_form() {
    for operation in OPERATIONS {
        for immediate in [false, true] {
            let mut code = vec![0x66; if immediate { 11 } else { 12 }];
            if immediate {
                code.extend_from_slice(&[0x0f, 0xba, 0xc0 | (operation.extension() << 3), 31]);
            } else {
                code.extend_from_slice(&[0x0f, operation.register_opcode(), 0xd0]);
            }
            let mut image = image(&[]);
            image.cpu.eip = 0x1ff1;
            image.cpu.registers.edx = 31;
            image.data(0x3ff1, &code);
            let result = expected(operation, 16, image.cpu.registers.eax, 31);
            let mut cpu = image.cpu;
            cpu.registers.eax = 0x4433_0000 | result.value;
            result.apply_flags(&mut cpu, prior_flags());
            cpu.eip = 0x2000;
            cpu.instruction_count = 0;
            both(
                TestModule::interpreter(),
                "fifteen-byte bit test or modification",
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
fn missing_bit_encoding_fields_fault_before_operand_and_flag_effects() {
    for operation in OPERATIONS {
        let group = operation.extension() << 3;
        for code in [
            vec![0x0f],
            vec![0x0f, operation.register_opcode()],
            vec![0x0f, operation.register_opcode(), 0x14],
            vec![0x66, 0x0f, operation.register_opcode(), 0x54, 0x8b],
            vec![0x0f, operation.register_opcode(), 0x15, 0x20, 0x40, 0],
            vec![0x66, 0x0f, 0xba, 0x44 | group, 0x8b, 0x80],
        ] {
            let start = 0x2000 - code.len() as u32;
            let mut image = image(&[]);
            image.cpu.eip = start;
            image.data(0x3000 + (start & 0xfff), &code);
            check(
                TestModule::interpreter(),
                "bit operation fetches its encoding before any effect",
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
fn length_limit_precedes_fetching_another_bit_encoding_field() {
    for operation in OPERATIONS {
        for (prefixes, suffix) in [
            (14, vec![0x0f]),
            (13, vec![0x0f, operation.register_opcode()]),
            (12, vec![0x0f, operation.register_opcode(), 0x14]),
            (11, vec![0x0f, operation.register_opcode(), 0x54, 0x8b]),
            (12, vec![0x0f, 0xba, 0xc0 | (operation.extension() << 3)]),
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
                "bit operation would exceed fifteen bytes",
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

#[test]
fn unsupported_ba_extensions_are_rejected_before_address_or_immediate_fetches() {
    for extension in 0..4 {
        for rm in [0xc0, 0x04] {
            let code = [0x0f, 0xba, rm | (extension << 3)];
            assert!(matches!(
                compile_block_from_bytes(0x1ffd, &code, 1),
                Err(BlockError::UnsupportedInstruction {
                    address: 0x1ffd,
                    opcode: 0x0f
                })
            ));
            let mut image = image(&[]);
            image.cpu.eip = 0x1ffd;
            image.data(0x3ffd, &code);
            check(
                TestModule::interpreter(),
                "unsupported BA extension needs no further bytes",
                &image,
                &[Step {
                    cpu: image.cpu,
                    ram: &[],
                    exit: Exit::Other(0x0008_000f_0000_1ffd),
                }],
            );
        }
    }
}

#[test]
fn lock_prefixed_bit_operations_remain_outside_the_supported_subset() {
    for operation in OPERATIONS {
        let code = [0xf0, 0x0f, operation.register_opcode(), 0x13];
        assert!(matches!(
            compile_block_from_bytes(0x1000, &code, 1),
            Err(BlockError::UnsupportedInstruction {
                address: 0x1000,
                opcode: 0xf0
            })
        ));
        let image = image(&code);
        check(
            TestModule::interpreter(),
            "LOCK bit operation is unsupported before any effect",
            &image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit: Exit::Other(0x0008_00f0_0000_1000),
            }],
        );
    }
}
