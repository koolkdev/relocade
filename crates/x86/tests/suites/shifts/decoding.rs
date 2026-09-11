use wasm86_x86::Gpr32::{Eax, Ecx};
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

use super::{image, STORED_FLAGS};

#[test]
fn snapshot_lengths_distinguish_implicit_cl_and_immediate_counts() {
    for code in [
        &[0xd0, 0xe4][..],
        &[0xd1, 0x2d, 0x20, 0x40, 0, 0][..],
        &[0xd2, 0xfc][..],
        &[0x66, 0xd3, 0xf9][..],
        &[0xc0, 0xed, 3][..],
        &[0x66, 0xc1, 0x64, 0x8b, 0xfc, 32][..],
    ] {
        for available in 0..code.len() {
            assert!(
                matches!(
                    compile_block_from_bytes(0x1000, &code[..available], 1),
                    Err(BlockError::TruncatedInstruction { address: 0x1000, available: actual })
                        if actual == available
                ),
                "{code:02x?}, available {available}"
            );
        }
        let complete = compile_block_from_bytes(0x1000, code, 1).unwrap();
        let with_suffix = [code, &[0x0f]].concat();
        assert_eq!(
            compile_block_from_bytes(0x1000, &with_suffix, 1)
                .unwrap()
                .bytes,
            complete.bytes
        );
    }
}

#[test]
fn unsupported_groups_stop_before_address_or_count_bytes() {
    for opcode in [0xc0, 0xc1, 0xd0, 0xd1, 0xd2, 0xd3] {
        let code = [opcode, 0x34]; // Unsupported /6, missing SIB and possibly an immediate.
        assert!(matches!(
            compile_block_from_bytes(0x1ffe, &code, 1),
            Err(BlockError::UnsupportedInstruction { address: 0x1ffe, opcode: actual }) if actual == opcode
        ));
        let mut image = image(&[]);
        image.cpu.eip = 0x1ffe;
        image.data(0x3ffe, &code);
        check(
            TestModule::interpreter(),
            "undocumented group six remains unsupported",
            &image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit: Exit::Other(0x0008_0000_0000_1ffe | (u64::from(opcode) << 32)),
            }],
        );
    }
}

#[test]
fn instruction_length_limit_precedes_fetching_a_required_field() {
    for (prefixes, suffix) in [
        (14, &[0xd0][..]),
        (13, &[0xc1, 0xe0][..]),
        (13, &[0xd2, 0x24][..]),
        (12, &[0xd3, 0x6c, 0x8b][..]),
    ] {
        let code = [vec![0x66; prefixes], suffix.to_vec()].concat();
        assert!(matches!(
            compile_block_from_bytes(0x1ff1, &code, 1),
            Err(BlockError::InstructionTooLong { address: 0x1ff1 })
        ));
        let mut image = image(&[]);
        image.cpu.eip = 0x1ff1;
        image.data(0x3ff1, &code);
        check(
            TestModule::interpreter(),
            "a required shift field would exceed fifteen bytes",
            &image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit: Exit::Other(0x0002_0000_0000_0000),
            }],
        );
    }
}

#[test]
fn required_fields_fault_before_operand_effects() {
    for code in [
        &[0xd0][..],
        &[0xd2, 0x64][..],
        &[0x66, 0xd3, 0x6c, 0x8b][..],
        &[0xc0, 0xe4][..],
        &[0xc1, 0x25, 0x20, 0x40, 0][..],
        &[0x66, 0xc1, 0x64, 0x8b, 0x80][..],
    ] {
        let start = 0x2000 - code.len() as u32;
        let mut image = image(&[]);
        image.cpu.eip = start;
        image.data(0x3000 + (start & 0xfff), code);
        check(
            TestModule::interpreter(),
            "missing shift encoding bytes precede data and flag effects",
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

#[rustfmt::skip]
fn page_end_cases() -> Vec<Case> {
    vec![
        Case::replacing_flags("SHL AL,1 at the page end", &[0xd0, 0xe0],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Clear })
            .stored_flags(STORED_FLAGS)
            .register(Eax, 0x4433_2211, 0x4433_2222).initial_register(Ecx, 0x8877_6620)
            .at(0x1ffe),
        Case::replacing_flags("SHL EAX,1 at the page end", &[0xd1, 0xe0],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Set, of: Set })
            .stored_flags(STORED_FLAGS)
            .register(Eax, 0x4433_2211, 0x8866_4422).initial_register(Ecx, 0x8877_6620)
            .at(0x1ffe),
        Case::preserving_flags("SHL AL,CL at the page end", &[0xd2, 0xe0])
            .stored_flags(STORED_FLAGS)
            .register(Eax, 0x4433_2211, 0x4433_2211).initial_register(Ecx, 0x8877_6620)
            .at(0x1ffe),
        Case::preserving_flags("SHL EAX,CL at the page end", &[0xd3, 0xe0])
            .stored_flags(STORED_FLAGS)
            .register(Eax, 0x4433_2211, 0x4433_2211).initial_register(Ecx, 0x8877_6620)
            .at(0x1ffe),
    ]
}

#[rustfmt::skip]
fn fifteen_byte_cases() -> Vec<Case> {
    vec![
        Case::preserving_flags("SHL AX,32 completes at byte fifteen",
            &[0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0xc1, 0xe0, 0x20])
            .stored_flags(STORED_FLAGS)
            .initial_register(Eax, 0x4433_2211)
            .at(0x1ff1),
    ]
}

test_cases!(page_end_forms, page_end_cases());
test_cases!(fifteen_byte_forms, fifteen_byte_cases());
