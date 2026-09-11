use wasm86_x86::{compile_block_from_bytes, BlockError, Gpr32};

use crate::support::{
    machine::{check, Exit, Step},
    step::TestModule,
};

use super::{image, EVEN, ODD, OPERATIONS, ZERO};
use crate::support::cases::{test_cases, InstructionCase as Case, Permissions::ReadOnly};

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

fn complete_encoding_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (prefix, source, first, last, flags) in [
        (&[0x66][..], 0_u32, 0x4433_a55b, 0x4433_a55b, ZERO),
        (&[0x66][..], 0x8000_8008, 0x4433_0003, 0x4433_000f, EVEN),
        (&[][..], 0, 0x4433_a55b, 0x4433_a55b, ZERO),
        (&[][..], 0x8000_8008, 3, 31, ODD),
    ] {
        for (opcode, result) in [(0xbc, first), (0xbd, last)] {
            for memory in [false, true] {
                let code = [prefix, &[0x0f, opcode, if memory { 0x03 } else { 0xc2 }]].concat();
                cases.push(
                    Case::replacing_flags(
                        format!("scan {code:02x?} ends at page boundary, source {source:x}"),
                        &code,
                        flags,
                    )
                    .at(0x2000 - code.len() as u32)
                    .register(Gpr32::Eax, 0x4433_a55b, result)
                    .initial_register(Gpr32::Edx, source)
                    .initial_register(Gpr32::Ebx, 0x4000)
                    .map_page(4, 0x8000, ReadOnly)
                    .backing(0x8000, &source.to_le_bytes()),
                );
            }
        }
    }
    for opcode in [0xbc, 0xbd] {
        for (source, result, flags) in [(0, 0x4433_a55b, ZERO), (0x8000, 0x4433_000f, ODD)] {
            let mut code = vec![0x66; 12];
            code.extend_from_slice(&[0x0f, opcode, 0xc2]);
            cases.push(
                Case::replacing_flags(
                    format!("fifteen-byte scan {opcode:x}, source {source:x}"),
                    &code,
                    flags,
                )
                .at(0x1ff1)
                .register(Gpr32::Eax, 0x4433_a55b, result)
                .initial_register(Gpr32::Edx, source),
            );
        }
    }
    cases
}
test_cases!(
    complete_encodings_at_page_and_length_boundaries,
    complete_encoding_cases()
);

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
