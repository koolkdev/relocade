use crate::support::encoding::check_length;
use wasm86_x86::Gpr32::{Eax, Ecx};
use wasm86_x86::{compile_block_from_bytes, BlockError};

use crate::support::{
    cases::{
        test_cases,
        FlagExpectation::{Clear, Preserved, Set},
        Flags, InstructionCase as Case,
    },
    machine::{check, Exit, Step},
    step::TestModule,
};

use super::{image, stored_flags, OPERATIONS};

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
            check_length(&code);
        }
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

const INITIAL: Flags<bool> = Flags {
    cf: true,
    pf: false,
    af: true,
    zf: true,
    sf: false,
    of: true,
};

#[rustfmt::skip]
fn page_end_cases() -> Vec<Case> {
    vec![
        Case::new("RCL AL,1 at the page end", &[0xd0, 0xd0], INITIAL,
            Flags { cf: Clear, of: Clear, ..Flags::all(Preserved) })
            .stored_flags(stored_flags(1))
            .register(Eax, 0x4433_2211, 0x4433_2223).initial_register(Ecx, 0x8877_6620)
            .at(0x1ffe),
        Case::new("RCL EAX,1 at the page end", &[0xd1, 0xd0], INITIAL,
            Flags { cf: Clear, of: Set, ..Flags::all(Preserved) })
            .stored_flags(stored_flags(1))
            .register(Eax, 0x4433_2211, 0x8866_4423).initial_register(Ecx, 0x8877_6620)
            .at(0x1ffe),
        Case::preserving_flags("RCL AL,CL at the page end", &[0xd2, 0xd0])
            .stored_flags(stored_flags(1))
            .register(Eax, 0x4433_2211, 0x4433_2211).initial_register(Ecx, 0x8877_6620)
            .at(0x1ffe),
        Case::preserving_flags("RCL EAX,CL at the page end", &[0xd3, 0xd0])
            .stored_flags(stored_flags(1))
            .register(Eax, 0x4433_2211, 0x4433_2211).initial_register(Ecx, 0x8877_6620)
            .at(0x1ffe),
        Case::new("RCR AL,1 at the page end", &[0xd0, 0xd8], INITIAL,
            Flags { cf: Set, of: Set, ..Flags::all(Preserved) })
            .stored_flags(stored_flags(1))
            .register(Eax, 0x4433_2211, 0x4433_2288).initial_register(Ecx, 0x8877_6620)
            .at(0x1ffe),
        Case::new("RCR EAX,1 at the page end", &[0xd1, 0xd8], INITIAL,
            Flags { cf: Set, of: Set, ..Flags::all(Preserved) })
            .stored_flags(stored_flags(1))
            .register(Eax, 0x4433_2211, 0xa219_9108).initial_register(Ecx, 0x8877_6620)
            .at(0x1ffe),
        Case::preserving_flags("RCR AL,CL at the page end", &[0xd2, 0xd8])
            .stored_flags(stored_flags(1))
            .register(Eax, 0x4433_2211, 0x4433_2211).initial_register(Ecx, 0x8877_6620)
            .at(0x1ffe),
        Case::preserving_flags("RCR EAX,CL at the page end", &[0xd3, 0xd8])
            .stored_flags(stored_flags(1))
            .register(Eax, 0x4433_2211, 0x4433_2211).initial_register(Ecx, 0x8877_6620)
            .at(0x1ffe),
    ]
}

#[rustfmt::skip]
fn fifteen_byte_cases() -> Vec<Case> {
    vec![
        Case::preserving_flags("RCL AX,32 completes at byte fifteen",
            &[0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0xc1, 0xd0, 0x20])
            .stored_flags(stored_flags(1))
            .initial_register(Eax, 0x4433_2211)
            .at(0x1ff1),
        Case::preserving_flags("RCR AX,32 completes at byte fifteen",
            &[0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0xc1, 0xd8, 0x20])
            .stored_flags(stored_flags(1))
            .initial_register(Eax, 0x4433_2211)
            .at(0x1ff1),
    ]
}

test_cases!(page_end_forms, page_end_cases());
test_cases!(fifteen_byte_forms, fifteen_byte_cases());
