use wasm86_x86::Gpr32::{Eax, Ecx, Edx};
use wasm86_x86::{compile_block_from_bytes, BlockError};

use crate::support::{
    cases::{
        test_cases,
        FlagExpectation::{Clear, Set},
        Flags, InstructionCase as Case,
    },
    machine::{check, Exit, Step},
    step::TestModule,
};

use super::{image, OPERATIONS, STORED_FLAGS};

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

#[rustfmt::skip]
fn page_end_cases() -> Vec<Case> {
    vec![
        Case::replacing_flags("SHLD AX,DX,CL at the page end", &[0x66, 0x0f, 0xa5, 0xd0],
            Flags { cf: Clear, pf: Clear, af: Clear, zf: Clear, sf: Clear, of: Clear })
            .stored_flags(STORED_FLAGS)
            .register(Eax, 0x4433_2211, 0x4433_4423)
            .initial_registers(&[(Ecx, 0x8877_6601), (Edx, 0xccbb_aa99)])
            .at(0x1ffc),
        Case::replacing_flags("SHLD EAX,EDX,CL at the page end", &[0x0f, 0xa5, 0xd0],
            Flags { cf: Clear, pf: Clear, af: Clear, zf: Clear, sf: Set, of: Set })
            .stored_flags(STORED_FLAGS)
            .register(Eax, 0x4433_2211, 0x8866_4423)
            .initial_registers(&[(Ecx, 0x8877_6601), (Edx, 0xccbb_aa99)])
            .at(0x1ffd),
        Case::replacing_flags("SHRD AX,DX,CL at the page end", &[0x66, 0x0f, 0xad, 0xd0],
            Flags { cf: Set, pf: Clear, af: Clear, zf: Clear, sf: Set, of: Set })
            .stored_flags(STORED_FLAGS)
            .register(Eax, 0x4433_2211, 0x4433_9108)
            .initial_registers(&[(Ecx, 0x8877_6601), (Edx, 0xccbb_aa99)])
            .at(0x1ffc),
        Case::replacing_flags("SHRD EAX,EDX,CL at the page end", &[0x0f, 0xad, 0xd0],
            Flags { cf: Set, pf: Clear, af: Clear, zf: Clear, sf: Set, of: Set })
            .stored_flags(STORED_FLAGS)
            .register(Eax, 0x4433_2211, 0xa219_9108)
            .initial_registers(&[(Ecx, 0x8877_6601), (Edx, 0xccbb_aa99)])
            .at(0x1ffd),
    ]
}

#[rustfmt::skip]
fn fifteen_byte_cases() -> Vec<Case> {
    vec![
        Case::preserving_flags("SHLD AX,DX,32 completes at byte fifteen",
            &[0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x0f, 0xa4, 0xd0, 0x20])
            .stored_flags(STORED_FLAGS)
            .initial_register(Eax, 0x4433_2211)
            .initial_registers(&[(Ecx, 0x8877_6620), (Edx, 0xccbb_aa99)])
            .at(0x1ff1),
        Case::preserving_flags("SHLD AX,DX,CL completes at byte fifteen",
            &[0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x0f, 0xa5, 0xd0])
            .stored_flags(STORED_FLAGS)
            .initial_register(Eax, 0x4433_2211)
            .initial_registers(&[(Ecx, 0x8877_6620), (Edx, 0xccbb_aa99)])
            .at(0x1ff1),
        Case::preserving_flags("SHRD AX,DX,32 completes at byte fifteen",
            &[0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x0f, 0xac, 0xd0, 0x20])
            .stored_flags(STORED_FLAGS)
            .initial_register(Eax, 0x4433_2211)
            .initial_registers(&[(Ecx, 0x8877_6620), (Edx, 0xccbb_aa99)])
            .at(0x1ff1),
        Case::preserving_flags("SHRD AX,DX,CL completes at byte fifteen",
            &[0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x0f, 0xad, 0xd0])
            .stored_flags(STORED_FLAGS)
            .initial_register(Eax, 0x4433_2211)
            .initial_registers(&[(Ecx, 0x8877_6620), (Edx, 0xccbb_aa99)])
            .at(0x1ff1),
    ]
}

test_cases!(page_end_forms, page_end_cases());
test_cases!(fifteen_byte_forms, fifteen_byte_cases());
