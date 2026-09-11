use wasm86_x86::{compile_block_from_bytes, BlockError, Gpr32};

use crate::support::{
    machine::{both, check, Exit, Step},
    step::TestModule,
};

use super::{expected, image, retire, OPERATIONS};

#[test]
fn bit_scans_decode_the_complete_register_or_memory_source_without_an_immediate() {
    for operation in OPERATIONS {
        for code in [
            vec![0x0f, operation.opcode(), 0xc2],
            vec![0x66, 0x0f, operation.opcode(), 0xed],
            vec![0x0f, operation.opcode(), 0x03],
            vec![0x66, 0x0f, operation.opcode(), 0x05, 0x20, 0x40, 0, 0],
            vec![0x0f, operation.opcode(), 0x44, 0x8b, 0xfc],
            vec![0x66, 0x0f, operation.opcode(), 0x84, 0x8b, 0x20, 0x40, 0, 0],
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
fn bit_scan_encodings_can_end_at_the_last_mapped_instruction_byte() {
    for operation in OPERATIONS {
        for bits in [16, 32] {
            for memory in [false, true] {
                for source in [0_u32, 0x8000_8008] {
                    let mut code = if bits == 16 { vec![0x66] } else { vec![] };
                    code.extend_from_slice(&[
                        0x0f,
                        operation.opcode(),
                        if memory { 0x03 } else { 0xc2 },
                    ]);
                    let start = 0x2000 - code.len() as u32;
                    let mut image = image(&[]);
                    image.cpu.eip = start;
                    image.data(0x3000 + (start & 0xfff), &code);
                    image.cpu.registers.edx = source;
                    image.cpu.registers.ebx = 0x4000;
                    image.map(4, 0x8000, false);
                    image.data(0x8000, &source.to_le_bytes());
                    let mut cpu = image.cpu;
                    expected(operation, bits, source, cpu.registers.eax)
                        .apply(&mut cpu, Gpr32::Eax);
                    let step = retire(&mut cpu, code.len() as u32);
                    both(
                        TestModule::interpreter(),
                        &format!("{operation:?} {bits}-bit source {source:x}, memory {memory}, ends at page boundary"),
                        &code,
                        1,
                        &image,
                        &[step],
                    );
                }
            }
        }
    }
}

#[test]
fn the_fifteenth_byte_can_complete_a_bit_scan() {
    for operation in OPERATIONS {
        for source in [0, 0x8000] {
            let mut code = vec![0x66; 12];
            code.extend_from_slice(&[0x0f, operation.opcode(), 0xc2]);
            let mut image = image(&[]);
            image.cpu.eip = 0x1ff1;
            image.cpu.registers.edx = source;
            image.data(0x3ff1, &code);
            let mut cpu = image.cpu;
            expected(operation, 16, source, cpu.registers.eax).apply(&mut cpu, Gpr32::Eax);
            let step = retire(&mut cpu, 15);
            both(
                TestModule::interpreter(),
                "fifteen-byte bit scan",
                &code,
                1,
                &image,
                &[step],
            );
        }
    }
}

#[test]
fn missing_scan_fields_fault_before_source_access_destination_changes_or_flags() {
    for operation in OPERATIONS {
        for code in [
            vec![0x0f],
            vec![0x0f, operation.opcode()],
            vec![0x0f, operation.opcode(), 0x04],
            vec![0x66, 0x0f, operation.opcode(), 0x44, 0x8b],
            vec![0x0f, operation.opcode(), 0x05, 0x20, 0x40, 0],
        ] {
            let start = 0x2000 - code.len() as u32;
            let mut image = image(&[]);
            image.cpu.eip = start;
            image.cpu.registers.edx = 0;
            image.data(0x3000 + (start & 0xfff), &code);
            check(
                TestModule::interpreter(),
                "bit scan fetch fault precedes operand effects",
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
fn scan_length_limit_precedes_fetching_a_sixteenth_encoding_byte() {
    for operation in OPERATIONS {
        for (prefixes, suffix) in [
            (14, vec![0x0f]),
            (13, vec![0x0f, operation.opcode()]),
            (12, vec![0x0f, operation.opcode(), 0x04]),
            (11, vec![0x0f, operation.opcode(), 0x44, 0x8b]),
            (9, vec![0x0f, operation.opcode(), 0x05, 0x20, 0x40, 0]),
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
                "bit scan needs a field beyond byte fifteen",
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
fn f3_prefixed_count_instructions_are_not_accepted_as_bit_scans() {
    for operation in OPERATIONS {
        for word in [false, true] {
            let mut code = if word { vec![0x66] } else { vec![] };
            code.extend_from_slice(&[0xf3, 0x0f, operation.opcode(), 0x03]);
            assert!(matches!(
                compile_block_from_bytes(0x1000, &code, 1),
                Err(BlockError::UnsupportedInstruction {
                    address: 0x1000,
                    opcode: 0xf3,
                })
            ));
            let image = image(&code);
            check(
                TestModule::interpreter(),
                "unsupported F3 prefix precedes the unmapped source",
                &image,
                &[Step {
                    cpu: image.cpu,
                    ram: &[],
                    exit: Exit::Other(0x0008_00f3_0000_1000),
                }],
            );
        }
    }
}
