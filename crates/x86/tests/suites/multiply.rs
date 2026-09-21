use wasm86_x86::Gpr32::{Eax, Ebx, Ecx, Edx, Esi, Esp};

use crate::support::{
    cases::{
        test_cases,
        FlagExpectation::{self, Clear, Set},
        Flags, InstructionCase as Case,
        Permissions::ReadOnly,
    },
    encoding::check_length,
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Sequence},
};

// CF and OF both report whether the product fits the low destination width.
// PF, AF, ZF and SF are architecturally undefined after MUL and IMUL.
fn product_flags(overflow: FlagExpectation) -> Flags<FlagExpectation> {
    Flags {
        cf: overflow,
        pf: FlagExpectation::Undefined,
        af: FlagExpectation::Undefined,
        zf: FlagExpectation::Undefined,
        sf: FlagExpectation::Undefined,
        of: overflow,
    }
}

#[rustfmt::skip]
fn full_products() -> Vec<Case> {
    vec![
        Case::replacing_flags("MUL byte maximum fits", &[0xf6, 0xe3], product_flags(Clear))
            .register(Eax, 0x4433_2201, 0x4433_00ff).initial_register(Ebx, 0x10ff_eeff),
        Case::replacing_flags("MUL byte maximum squared", &[0xf6, 0xe3], product_flags(Set))
            .register(Eax, 0x4433_22ff, 0x4433_fe01).initial_register(Ebx, 0x10ff_eeff),
        Case::replacing_flags("MUL byte first product above the width", &[0xf6, 0xe3], product_flags(Set))
            .register(Eax, 0x4433_2280, 0x4433_0100).initial_register(Ebx, 0x10ff_ee02),
        Case::replacing_flags("MUL byte unsigned sign bit still fits", &[0xf6, 0xe3], product_flags(Clear))
            .register(Eax, 0x4433_227f, 0x4433_00fe).initial_register(Ebx, 0x10ff_ee02),
        Case::replacing_flags("IMUL byte minimum fits", &[0xf6, 0xeb], product_flags(Clear))
            .register(Eax, 0x4433_2280, 0x4433_ff80).initial_register(Ebx, 0x10ff_ee01),
        Case::replacing_flags("IMUL byte negating the minimum overflows", &[0xf6, 0xeb], product_flags(Set))
            .register(Eax, 0x4433_2280, 0x4433_0080).initial_register(Ebx, 0x10ff_eeff),
        Case::replacing_flags("IMUL byte positive result needs another sign bit", &[0xf6, 0xeb], product_flags(Set))
            .register(Eax, 0x4433_227f, 0x4433_00fe).initial_register(Ebx, 0x10ff_ee02),
        Case::replacing_flags("MUL word maximum fits", &[0x66, 0xf7, 0xe3], product_flags(Clear))
            .register(Eax, 0x4433_0001, 0x4433_ffff).register(Edx, 0xccbb_aa99, 0xccbb_0000)
            .initial_register(Ebx, 0x10ff_ffff),
        Case::replacing_flags("MUL word first product above the width", &[0x66, 0xf7, 0xe3], product_flags(Set))
            .register(Eax, 0x4433_8000, 0x4433_0000).register(Edx, 0xccbb_aa99, 0xccbb_0001)
            .initial_register(Ebx, 0x10ff_0002),
        Case::replacing_flags("MUL word unsigned sign bit still fits", &[0x66, 0xf7, 0xe3], product_flags(Clear))
            .register(Eax, 0x4433_7fff, 0x4433_fffe).register(Edx, 0xccbb_aa99, 0xccbb_0000)
            .initial_register(Ebx, 0x10ff_0002),
        Case::replacing_flags("IMUL word minimum fits", &[0x66, 0xf7, 0xeb], product_flags(Clear))
            .register(Eax, 0x4433_8000, 0x4433_8000).register(Edx, 0xccbb_aa99, 0xccbb_ffff)
            .initial_register(Ebx, 0x10ff_0001),
        Case::replacing_flags("IMUL word negating the minimum overflows", &[0x66, 0xf7, 0xeb], product_flags(Set))
            .register(Eax, 0x4433_8000, 0x4433_8000).register(Edx, 0xccbb_aa99, 0xccbb_0000)
            .initial_register(Ebx, 0x10ff_ffff),
        Case::replacing_flags("IMUL word positive result needs another sign bit", &[0x66, 0xf7, 0xeb], product_flags(Set))
            .register(Eax, 0x4433_7fff, 0x4433_fffe).register(Edx, 0xccbb_aa99, 0xccbb_0000)
            .initial_register(Ebx, 0x10ff_0002),
        Case::replacing_flags("MUL dword maximum fits", &[0xf7, 0xe3], product_flags(Clear))
            .register(Eax, 1, 0xffff_ffff).register(Edx, 0xccbb_aa99, 0).initial_register(Ebx, 0xffff_ffff),
        Case::replacing_flags("MUL dword first product above the width", &[0xf7, 0xe3], product_flags(Set))
            .register(Eax, 0x8000_0000, 0).register(Edx, 0xccbb_aa99, 1).initial_register(Ebx, 2),
        Case::replacing_flags("MUL dword unsigned sign bit still fits", &[0xf7, 0xe3], product_flags(Clear))
            .register(Eax, 0x7fff_ffff, 0xffff_fffe).register(Edx, 0xccbb_aa99, 0).initial_register(Ebx, 2),
        Case::replacing_flags("IMUL dword minimum fits", &[0xf7, 0xeb], product_flags(Clear))
            .register(Eax, 0x8000_0000, 0x8000_0000).register(Edx, 0xccbb_aa99, 0xffff_ffff)
            .initial_register(Ebx, 1),
        Case::replacing_flags("IMUL dword negating the minimum overflows", &[0xf7, 0xeb], product_flags(Set))
            .register(Eax, 0x8000_0000, 0x8000_0000).register(Edx, 0xccbb_aa99, 0)
            .initial_register(Ebx, 0xffff_ffff),
        Case::replacing_flags("IMUL dword positive result needs another sign bit", &[0xf7, 0xeb], product_flags(Set))
            .register(Eax, 0x7fff_ffff, 0xffff_fffe).register(Edx, 0xccbb_aa99, 0).initial_register(Ebx, 2),
        Case::replacing_flags("IMUL byte captures negative AH before replacing AX", &[0xf6, 0xec], product_flags(Clear))
            .register(Eax, 0x4433_ff03, 0x4433_fffd),
        Case::replacing_flags("IMUL byte captures negative AL before squaring it", &[0xf6, 0xe8], product_flags(Clear))
            .register(Eax, 0x4433_22ff, 0x4433_0001),
        Case::replacing_flags("MUL word captures maximum DX before replacing both halves", &[0x66, 0xf7, 0xe2], product_flags(Set))
            .register(Eax, 0x4433_ffff, 0x4433_0001).register(Edx, 0xccbb_ffff, 0xccbb_fffe),
        Case::replacing_flags("IMUL word old DX produces a negative high half", &[0x66, 0xf7, 0xea], product_flags(Set))
            .register(Eax, 0x4433_8000, 0x4433_0000).register(Edx, 0xccbb_0002, 0xccbb_ffff),
        Case::replacing_flags("IMUL word captures minimum AX before squaring it", &[0x66, 0xf7, 0xe8], product_flags(Set))
            .register(Eax, 0x4433_8000, 0x4433_0000).register(Edx, 0xccbb_aa99, 0xccbb_4000),
        Case::replacing_flags("MUL dword captures maximum EDX before replacing both halves", &[0xf7, 0xe2], product_flags(Set))
            .register(Eax, 0xffff_ffff, 1).register(Edx, 0xffff_ffff, 0xffff_fffe),
        Case::replacing_flags("IMUL dword old EDX produces a negative high half", &[0xf7, 0xea], product_flags(Set))
            .register(Eax, 0x8000_0000, 0).register(Edx, 2, 0xffff_ffff),
        Case::replacing_flags("IMUL dword captures minimum EAX before squaring it", &[0xf7, 0xe8], product_flags(Set))
            .register(Eax, 0x8000_0000, 0).register(Edx, 0xccbb_aa99, 0x4000_0000),
    ]
}

#[rustfmt::skip]
fn explicit_products() -> Vec<Case> {
    vec![
        Case::replacing_flags("two-operand word IMUL negating the minimum overflows", &[0x66, 0x0f, 0xaf, 0xc3], product_flags(Set))
            .register(Eax, 0x4433_8000, 0x4433_8000).initial_register(Ebx, 0x10ff_ffff),
        Case::replacing_flags("two-operand dword IMUL multiplies two negative operands", &[0x0f, 0xaf, 0xc3], product_flags(Clear))
            .register(Eax, 0xffff_fffd, 6).initial_register(Ebx, 0xffff_fffe),
        Case::replacing_flags("two-operand word IMUL captures the old self operand", &[0x66, 0x0f, 0xaf, 0xc0], product_flags(Clear))
            .register(Eax, 0x4433_fffd, 0x4433_0009),
        Case::replacing_flags("two-operand dword IMUL zero low half still overflows", &[0x0f, 0xaf, 0xc0], product_flags(Set))
            .register(Eax, 0x8000_0000, 0),
        Case::replacing_flags("word IMUL full immediate 0080 is positive and ignores old destination", &[0x66, 0x69, 0xc3, 0x80, 0], product_flags(Set))
            .register(Eax, 0x4433_dead, 0x4433_8000).initial_register(Ebx, 0x10ff_0100),
        Case::replacing_flags("dword IMUL full immediate 00000080 is positive", &[0x69, 0xc3, 0x80, 0, 0, 0], product_flags(Set))
            .register(Eax, 0x4433_dead, 0x8000_0000).initial_register(Ebx, 0x0100_0000),
        Case::replacing_flags("dword IMUL full negative immediate", &[0x69, 0xc3, 0xfe, 0xff, 0xff, 0xff], product_flags(Clear))
            .register(Eax, 0x4433_dead, 0xffff_fffa).initial_register(Ebx, 3),
        Case::replacing_flags("word IMUL signed byte 7f stays positive", &[0x66, 0x6b, 0xc3, 0x7f], product_flags(Clear))
            .register(Eax, 0x4433_dead, 0x4433_7f00).initial_register(Ebx, 0x10ff_0100),
        Case::replacing_flags("word IMUL signed byte 80 reaches the negative limit", &[0x66, 0x6b, 0xc3, 0x80], product_flags(Clear))
            .register(Eax, 0x4433_dead, 0x4433_8000).initial_register(Ebx, 0x10ff_0100),
        Case::replacing_flags("dword IMUL signed byte 80 overflows below the negative limit", &[0x6b, 0xc3, 0x80], product_flags(Set))
            .register(Eax, 0x4433_dead, 0x7fff_ff80).initial_register(Ebx, 0x0100_0001),
        Case::replacing_flags("dword IMUL signed byte ff reads its old destination as source", &[0x6b, 0xc0, 0xff], product_flags(Clear))
            .register(Eax, 0xffff_ffff, 1),
    ]
}

#[rustfmt::skip]
fn memory_sources() -> Vec<Case> {
    vec![
        Case::replacing_flags("MUL byte reads only the final mapped byte", &[0xf6, 0x23], product_flags(Set))
            .register(Eax, 0x4433_22f0, 0x4433_01e0).initial_register(Ebx, 0x4fff)
            .memory(0x4fff, &[2], ReadOnly),
        Case::replacing_flags("IMUL word reads a source across scattered pages", &[0x66, 0xf7, 0x2b], product_flags(Set))
            .register(Eax, 0x4433_8000, 0x4433_0000).register(Edx, 0xccbb_aa99, 0xccbb_ffff)
            .initial_register(Ebx, 0x4fff).map_page(4, 0x8000, ReadOnly).map_page(5, 0xa000, ReadOnly)
            .memory(0x4fff, &[2, 0], ReadOnly),
        Case::replacing_flags("MUL byte captures the EAX address before replacing AX", &[0xf6, 0x20], product_flags(Set))
            .register(Eax, 0x4083, 0x0106)
            .memory(0x4082, &[0x5a, 2, 0xa5], ReadOnly),
        Case::replacing_flags("IMUL dword captures its EDX source address", &[0xf7, 0x2a], product_flags(Clear))
            .register(Eax, 0xffff_fffd, 3).register(Edx, 0x4020, 0)
            .memory(0x401f, &[0x5a, 0xff, 0xff, 0xff, 0xff, 0xa5], ReadOnly),
        Case::replacing_flags("IMUL two operands captures EDX as both old destination and address", &[0x0f, 0xaf, 0x12], product_flags(Clear))
            .register(Edx, 0x4020, 0xc060)
            .memory(0x401f, &[0x5a, 3, 0, 0, 0, 0xa5], ReadOnly),
        Case::replacing_flags("IMUL wide immediate reads EAX only as the source address", &[0x69, 0x00, 0xfe, 0xff, 0xff, 0xff], product_flags(Clear))
            .register(Eax, 0x4020, 0xffff_fffa)
            .memory(0x401f, &[0x5a, 3, 0, 0, 0, 0xa5], ReadOnly),
        Case::replacing_flags("IMUL signed immediate captures old ESP and wrapping scaled EAX", &[0x6b, 0x64, 0x84, 0xfc, 0xfe], product_flags(Clear))
            .register(Esp, 0x4010, 6).initial_register(Eax, 0x4000_0001)
            .memory(0x400f, &[0x5a, 0xfd, 0xff, 0xff, 0xff, 0xa5], ReadOnly),
    ]
}

#[rustfmt::skip]
fn source_faults() -> Vec<Case> {
    vec![
        Case::preserving_flags("MUL zero accumulator still reads its byte source", &[0xf6, 0x23])
            .initial_registers(&[(Eax, 0), (Ebx, 0x5000)]).fault(0x5000, 0),
        Case::preserving_flags("IMUL zero accumulator still reads its complete dword source", &[0xf7, 0x2b])
            .initial_registers(&[(Eax, 0), (Ebx, 0x4fff)])
            .memory(0x4fff, &[1], ReadOnly).fault(0x5000, 0),
        Case::preserving_flags("two-operand IMUL zero destination still reads its word source", &[0x66, 0x0f, 0xaf, 0x03])
            .initial_registers(&[(Eax, 0), (Ebx, 0x5000)]).fault(0x5000, 0),
        Case::preserving_flags("IMUL full immediate zero still reads its dword source", &[0x69, 0x03, 0, 0, 0, 0])
            .initial_register(Ebx, 0x5000).fault(0x5000, 0),
        Case::preserving_flags("IMUL byte immediate zero still reads its complete word source", &[0x66, 0x6b, 0x03, 0])
            .initial_register(Ebx, 0x4fff).memory(0x4fff, &[1], ReadOnly).fault(0x5000, 0),
    ]
}

// These exact undefined-flag values are a software policy, not x86 guarantees.
#[rustfmt::skip]
fn undefined_flags_policy() -> Vec<Case> {
    vec![
        Case::replacing_flags("MUL zero follows software policy with ZF clear", &[0xf6, 0xe3],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4433_2200, 0x4433_0000).initial_register(Ebx, 0x10ff_eeff),
        Case::replacing_flags("IMUL negative word follows software policy with PF set and SF clear", &[0x66, 0xf7, 0xeb],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4433_fffd, 0x4433_fffd).register(Edx, 0xccbb_aa99, 0xccbb_ffff)
            .initial_register(Ebx, 0x10ff_0001),
    ]
}

#[rustfmt::skip]
fn product_sequences() -> Vec<Sequence> {
    vec![
        Sequence::from_opaque_flags("MUL old AH produces carry and overflow for conditions and ADC")
            .initial_registers(&[(Eax, 0x4433_8080), (Ebx, 0x10ff_eedd), (Edx, 0x7fff_ffff)])
            .step(Step::new(&[0xf6, 0xe4], product_flags(Set)).register(Eax, 0x4433_4000))
            .step(Step::preserving_flags(&[0x0f, 0x92, 0xc3]).register(Ebx, 0x10ff_ee01))
            .step(Step::preserving_flags(&[0x0f, 0x90, 0xc7]).register(Ebx, 0x10ff_0101))
            .step(Step::new(&[0x83, 0xd2, 0],
                Flags { cf: Clear, pf: Set, af: Set, zf: Clear, sf: Set, of: Set })
                .register(Edx, 0x8000_0000)),
        Sequence::from_opaque_flags("signed products feed conditions and a later self multiply")
            .initial_registers(&[(Eax, 0xffff_fffe), (Edx, 3), (Ebx, 0x10ff_eedd)])
            .step(Step::new(&[0x0f, 0xaf, 0xc2], product_flags(Clear)).register(Eax, 0xffff_fffa))
            .step(Step::preserving_flags(&[0x0f, 0x92, 0xc3]).register(Ebx, 0x10ff_ee00))
            .step(Step::preserving_flags(&[0x0f, 0x90, 0xc7]).register(Ebx, 0x10ff_0000))
            .step(Step::new(&[0x0f, 0xaf, 0xc0], product_flags(Clear)).register(Eax, 0x24)),
        Sequence::from_opaque_flags("a zero immediate still faults and publishes the preceding ADD")
            .initial_registers(&[(Eax, 0x4433_22ff), (Edx, 0xccbb_aa01), (Esi, 0x5000)])
            .step(Step::new(&[0x00, 0xd0],
                Flags { cf: Set, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear })
                .register(Eax, 0x4433_2200))
            .step(Step::preserving_flags(&[0x69, 0x16, 0, 0, 0, 0]).fault(0x5000, 0))
            .trailing_code(&[0xb8, 0, 0, 0, 0], 1),
        Sequence::from_opaque_flags("a later source fault publishes both halves of the completed product")
            .initial_registers(&[(Eax, 0xffff_ffff), (Edx, 0xccbb_aa99), (Ebx, 3), (Ecx, 0x5000)])
            .step(Step::new(&[0xf7, 0xe3], product_flags(Set))
                .register(Eax, 0xffff_fffd).register(Edx, 2))
            .step(Step::preserving_flags(&[0x0f, 0xaf, 0x01]).fault(0x5000, 0))
            .trailing_code(&[0xba, 0, 0, 0, 0], 1),
    ]
}

#[test]
fn snapshots_require_the_source_fields_and_the_selected_immediate_width() {
    for code in [
        &[0xf6, 0xe4][..],
        &[0x66, 0xf7, 0x24, 0x8b][..],
        &[0xf7, 0x2d, 0x20, 0x40, 0, 0][..],
        &[0x0f, 0xaf, 0xc3][..],
        &[0x66, 0x0f, 0xaf, 0x84, 0x8b, 0x20, 0x40, 0, 0][..],
        &[0x69, 0xc0, 0xfe, 0xff, 0xff, 0xff][..],
        &[0x66, 0x69, 0x45, 0x80, 0xfe, 0xff][..],
        &[0x6b, 0xc7, 0xff][..],
        &[0x66, 0x6b, 0x84, 0x8b, 0x20, 0x40, 0, 0, 0x80][..],
        &[0x66, 0x66, 0xf6, 0xec][..],
        &[0x69, 0x04, 0x25, 0x20, 0x40, 0, 0, 0xfe, 0xff, 0xff, 0xff][..],
    ] {
        check_length(code);
    }
}

test_cases!(full_products_and_signed_fit, full_products());
test_cases!(explicit_forms_and_truncated_products, explicit_products());
test_cases!(memory_sources_and_address_aliases, memory_sources());
test_cases!(complete_reads_precede_product_effects, source_faults());
test_cases!(
    undefined_flags_follow_software_policy,
    undefined_flags_policy()
);
test_sequences!(
    products_conditions_and_fault_publication,
    product_sequences()
);
