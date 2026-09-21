//! BSWAP byte order and the chosen result for its undefined word form.

use crate::support::{
    cases::{
        test_cases,
        FlagExpectation::{Clear, Set},
        Flags, InstructionCase as Case,
    },
    encoding::check_length,
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Sequence},
};
use wasm86_x86::{Gpr32::*, Segment, SegmentAttributes, StoredSegment};

fn register_forms() -> Vec<Case> {
    [
        (0xc8, Eax),
        (0xc9, Ecx),
        (0xca, Edx),
        (0xcb, Ebx),
        (0xcc, Esp),
        (0xcd, Ebp),
        (0xce, Esi),
        (0xcf, Edi),
    ]
    .into_iter()
    .map(|(opcode, register)| {
        Case::preserving_flags(format!("BSWAP {register:?}"), &[0x0f, opcode]).register(
            register,
            0x9280_fe67,
            0x67fe_8092,
        )
    })
    .collect()
}

#[rustfmt::skip]
fn word_compatibility() -> Vec<Case> {
    vec![
        Case::preserving_flags("BSWAP AX clears the low word", &[0x66, 0x0f, 0xc8])
            .register(Eax, 0x1234_5678, 0x1234_0000),
        Case::preserving_flags("BSWAP SP preserves every upper bit", &[0x66, 0x0f, 0xcc])
            .register(Esp, 0xffff_ffff, 0xffff_0000),
        Case::preserving_flags("BSWAP DI keeps an already zero low word", &[0x66, 0x0f, 0xcf])
            .initial_register(Edi, 0x9234_0000),
    ]
}

fn code16_forms() -> Vec<Case> {
    [
        Case::preserving_flags(
            "CS.D=0 selects the word compatibility result",
            &[0x0f, 0xcb],
        )
        .register(Ebx, 0x1234_5678, 0x1234_0000),
        Case::preserving_flags(
            "66 selects byte reversal in 16-bit code",
            &[0x66, 0x0f, 0xca],
        )
        .register(Edx, 0x9280_fe67, 0x67fe_8092),
    ]
    .into_iter()
    .map(|case| {
        case.segmented_only().segment(
            Segment::Cs,
            StoredSegment {
                attributes: SegmentAttributes::from_bits(0x07),
                ..StoredSegment::flat_code32(0x1b)
            },
        )
    })
    .collect()
}

#[test]
fn forms_consume_only_the_opcode_and_operand_prefix() {
    for code in [&[0x0f, 0xc8][..], &[0x0f, 0xcf], &[0x66, 0x0f, 0xcc]] {
        check_length(code);
    }
}

test_cases!(all_dword_registers_reverse_bytes, register_forms());
test_cases!(word_form_clears_only_the_low_word, word_compatibility());
test_cases!(operand_size_follows_code_defaults, code16_forms());

#[rustfmt::skip]
fn alias_and_flag_sequences() -> Vec<Sequence> {
    vec![Sequence::from_opaque_flags("BSWAP uses preceding alias writes and preserves pending flags")
        .initial_registers(&[(Eax, 0x1234_5678), (Ebx, 0x7fff_ffff), (Ecx, 0)])
        .step(Step::new(&[0x83, 0xc3, 1],
            Flags { cf: Clear, pf: Set, af: Set, zf: Clear, sf: Set, of: Set }).register(Ebx, 0x8000_0000))
        .step(Step::preserving_flags(&[0xb4, 0x9a]).register(Eax, 0x1234_9a78))
        .step(Step::preserving_flags(&[0x0f, 0xc8]).register(Eax, 0x789a_3412))
        .step(Step::preserving_flags(&[0x66, 0x0f, 0xc8]).register(Eax, 0x789a_0000))
        .step(Step::preserving_flags(&[0x0f, 0xc8]).register(Eax, 0x0000_9a78))
        .step(Step::preserving_flags(&[0x0f, 0x90, 0xc1]).register(Ecx, 1))]
}

test_sequences!(alias_values_and_pending_flags, alias_and_flag_sequences());
