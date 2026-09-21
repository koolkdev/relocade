//! Carry and direction controls change only their named flag.
use crate::flags::Flag;
use crate::support::{
    cases::{
        test_cases,
        FlagExpectation::{self, Clear, Preserved, Set},
        Flags, InstructionCase as Case,
    },
    encoding::check_length,
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Sequence},
};
use wasm86_x86::{
    CpuState,
    Gpr32::{Eax, Ebx, Ecx, Esi},
};

fn carry_result(value: bool) -> Flags<FlagExpectation> {
    Flags {
        cf: if value { Set } else { Clear },
        ..Flags::all(Preserved)
    }
}

fn carry_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for initial in [false, true] {
        for (name, opcode, result) in [
            ("CLC", 0xf8, false),
            ("STC", 0xf9, true),
            ("CMC", 0xf5, !initial),
        ] {
            let code = if initial {
                vec![0x66, opcode]
            } else {
                vec![opcode]
            };
            cases.push(Case::new(
                format!("{name}, initial status flags {initial}"),
                &code,
                Flags::all(initial),
                carry_result(result),
            ));
        }
    }
    cases
}

fn direction_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for fill in [0x80, 0xff] {
        for (name, opcode, result) in [("CLD", 0xfc, false), ("STD", 0xfd, true)] {
            let code = if fill == 0xff {
                vec![0x66, opcode]
            } else {
                vec![opcode]
            };
            cases.push(
                Case::preserving_flags(format!("{name} preserves opaque {fill:02x} status"), &code)
                    .stored_flags(CpuState::filled(fill).flags)
                    .expect_direct_flag(Flag::DF, result),
            );
        }
    }
    cases
}

#[test]
fn controls_consume_only_their_opcode_and_ignored_operand_prefix() {
    for opcode in [0xf8, 0xf9, 0xf5, 0xfc, 0xfd] {
        check_length(&[opcode]);
        check_length(&[0x66, opcode]);
    }
}

#[rustfmt::skip]
fn dependent_flags() -> Vec<Sequence> {
    vec![
        Sequence::new("carry controls replace pending ADD flags before ADC and SBB", Flags::all(false))
            .initial_registers(&[(Eax, u32::MAX), (Ecx, 0)])
            .step(Step::new(&[0x05, 1, 0, 0, 0],
                Flags { cf: Set, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear }).register(Eax, 0))
            .step(Step::new(&[0xf8], carry_result(false)))
            .step(Step::new(&[0x83, 0xd1, 0],
                Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear }))
            .step(Step::new(&[0xf9], carry_result(true)))
            .step(Step::new(&[0x83, 0xd9, 0],
                Flags { cf: Set, pf: Set, af: Set, zf: Clear, sf: Set, of: Clear }).register(Ecx, u32::MAX))
            .step(Step::new(&[0xf5], carry_result(false)))
            .step(Step::preserving_flags(&[0x73, 5]).dispatch(0x1015))
            .trailing_code(&[0xf9, 0xfd], 2),
        Sequence::new("CMC consumes pending carry and preserves overflow for SETO", Flags::all(false))
            .initial_registers(&[(Eax, 0x4433_227f), (Ebx, 0x8877_6600), (Ecx, 0)])
            .step(Step::new(&[0x04, 1],
                Flags { cf: Clear, pf: Clear, af: Set, zf: Clear, sf: Set, of: Set }).register(Eax, 0x4433_2280))
            .step(Step::new(&[0xf5], carry_result(true)))
            .step(Step::preserving_flags(&[0x0f, 0x90, 0xc3]).register(Ebx, 0x8877_6601))
            .step(Step::preserving_flags(&[0x0f, 0x92, 0xc1]).register(Ecx, 1)),
        Sequence::new("direction changes preserve pending carry and publish before a later fault", Flags::all(false))
            .initial_registers(&[(Eax, u32::MAX), (Ecx, 0), (Esi, 0x6000)])
            .step(Step::new(&[0x05, 1, 0, 0, 0],
                Flags { cf: Set, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear }).register(Eax, 0))
            .step(Step::preserving_flags(&[0xfd]).expect_direct_flag(Flag::DF, true))
            .step(Step::preserving_flags(&[0xfc]).expect_direct_flag(Flag::DF, false))
            .step(Step::new(&[0x83, 0xd1, 0], Flags::all(Clear)).register(Ecx, 1))
            .step(Step::new(&[0xf9], carry_result(true)))
            .step(Step::preserving_flags(&[0xfd]).expect_direct_flag(Flag::DF, true))
            .step(Step::preserving_flags(&[0x89, 0x06]).fault(0x6000, 2))
            .trailing_code(&[0xf8, 0xfc], 2),
    ]
}

test_cases!(carry_changes_and_preserved_status, carry_cases());
test_cases!(direction_changes_preserve_opaque_status, direction_cases());
test_sequences!(pending_flags_consumers_and_publication, dependent_flags());
