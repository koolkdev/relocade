//! Sign boundaries and dividend consumers for the implicit accumulator extensions.
use crate::support::{
    cases::{
        test_cases,
        FlagExpectation::{Clear, Set, Undefined},
        Flags, InstructionCase as Case,
    },
    encoding::check_length,
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Sequence},
};
use wasm86_x86::Gpr32::{Eax, Ebx, Ecx, Edx};

#[rustfmt::skip]
fn boundary_cases() -> Vec<Case> {
    [
        ("CBW zero", &[0x66, 0x98][..], 0x4433_ff00, 0x4433_0000, 0xccbb_aa99),
        ("CBW positive maximum", &[0x66, 0x98], 0x4433_807f, 0x4433_007f, 0xccbb_aa99),
        ("CBW negative minimum", &[0x66, 0x98], 0x4433_7f80, 0x4433_ff80, 0xccbb_aa99),
        ("CBW negative one", &[0x66, 0x98], 0x4433_00ff, 0x4433_ffff, 0xccbb_aa99),
        ("CWDE zero", &[0x98], 0xffff_0000, 0, 0xccbb_aa99),
        ("CWDE positive maximum", &[0x98], 0xffff_7fff, 0x0000_7fff, 0xccbb_aa99),
        ("CWDE negative minimum", &[0x98], 0x0000_8000, 0xffff_8000, 0xccbb_aa99),
        ("CWDE negative one", &[0x98], 0x1234_ffff, 0xffff_ffff, 0xccbb_aa99),
        ("CWD zero", &[0x66, 0x99], 0xffff_0000, 0xffff_0000, 0xccbb_0000),
        ("CWD positive maximum", &[0x66, 0x99], 0xffff_7fff, 0xffff_7fff, 0xccbb_0000),
        ("CWD negative minimum", &[0x66, 0x99], 0x0000_8000, 0x0000_8000, 0xccbb_ffff),
        ("CWD negative one", &[0x66, 0x99], 0x1234_ffff, 0x1234_ffff, 0xccbb_ffff),
        ("CDQ zero", &[0x99], 0, 0, 0),
        ("CDQ positive maximum", &[0x99], 0x7fff_ffff, 0x7fff_ffff, 0),
        ("CDQ negative minimum", &[0x99], 0x8000_0000, 0x8000_0000, 0xffff_ffff),
        ("CDQ negative one", &[0x99], 0xffff_ffff, 0xffff_ffff, 0xffff_ffff),
    ].into_iter().map(|(name, code, input, eax, edx)| {
        Case::preserving_flags(name, code)
            .register(Eax, input, eax).register(Edx, 0xccbb_aa99, edx)
    }).collect()
}

#[test]
fn each_form_consumes_only_its_opcode_and_operand_prefix() {
    for code in [&[0x66, 0x98][..], &[0x98], &[0x66, 0x99], &[0x99]] {
        check_length(code);
    }
}

#[rustfmt::skip]
fn dividend_and_flag_sequences() -> Vec<Sequence> {
    vec![
        Sequence::from_opaque_flags("all four extensions preserve pending arithmetic for later conditions")
            .initial_registers(&[(Eax, 0x4433_7f80), (Edx, 0xccbb_0000), (Ebx, 0x7fff_ffff), (Ecx, 0)])
            .step(Step::new(&[0x83, 0xc3, 1],
                Flags { cf: Clear, pf: Set, af: Set, zf: Clear, sf: Set, of: Set }).register(Ebx, 0x8000_0000))
            .step(Step::preserving_flags(&[0x66, 0x98]).register(Eax, 0x4433_ff80))
            .step(Step::preserving_flags(&[0x98]).register(Eax, 0xffff_ff80))
            .step(Step::preserving_flags(&[0x66, 0x99]).register(Edx, 0xccbb_ffff))
            .step(Step::preserving_flags(&[0x99]).register(Edx, 0xffff_ffff))
            .step(Step::preserving_flags(&[0x0f, 0x90, 0xc1]).register(Ecx, 1))
            .step(Step::preserving_flags(&[0x0f, 0x92, 0xc5]).register(Ecx, 1)),
        Sequence::new("CBW prepares a negative byte dividend after MOV AL", Flags::all(true))
            .initial_registers(&[(Eax, 0x4433_2211), (Ecx, 7)])
            .step(Step::preserving_flags(&[0xb0, 0x9c]).register(Eax, 0x4433_229c))
            .step(Step::preserving_flags(&[0x66, 0x98]).register(Eax, 0x4433_ff9c))
            .step(Step::new(&[0xf6, 0xf9], Flags::all(Undefined)).register(Eax, 0x4433_fef2)),
        Sequence::new("CWD prepares DX while preserving EAX and the high EDX half", Flags::all(false))
            .initial_registers(&[(Eax, 0x4433_2211), (Edx, 0xccbb_aa99), (Ecx, 7)])
            .step(Step::preserving_flags(&[0x66, 0xb8, 0x9c, 0xff]).register(Eax, 0x4433_ff9c))
            .step(Step::preserving_flags(&[0x66, 0x99]).register(Edx, 0xccbb_ffff))
            .step(Step::new(&[0x66, 0xf7, 0xf9], Flags::all(Undefined))
                .register(Eax, 0x4433_fff2).register(Edx, 0xccbb_fffe)),
        Sequence::new("CWDE and CDQ prepare a dword dividend from a preceding AX write", Flags::all(true))
            .initial_registers(&[(Eax, 0x4433_2211), (Edx, 0xccbb_aa99), (Ecx, 7)])
            .step(Step::preserving_flags(&[0x66, 0xb8, 0x9c, 0xff]).register(Eax, 0x4433_ff9c))
            .step(Step::preserving_flags(&[0x98]).register(Eax, 0xffff_ff9c))
            .step(Step::preserving_flags(&[0x99]).register(Edx, 0xffff_ffff))
            .step(Step::new(&[0xf7, 0xf9], Flags::all(Undefined))
                .register(Eax, 0xffff_fff2).register(Edx, 0xffff_fffe)),
        Sequence::preserving_flags("dword quotient overflow publishes the preceding MOV and CDQ")
            .initial_registers(&[(Eax, 0x4433_2211), (Edx, 0xccbb_aa99), (Ecx, 0xffff_ffff)])
            .step(Step::preserving_flags(&[0xb8, 0, 0, 0, 0x80]).register(Eax, 0x8000_0000))
            .step(Step::preserving_flags(&[0x99]).register(Edx, 0xffff_ffff))
            .step(Step::preserving_flags(&[0xf7, 0xf9]).divide_error())
            .trailing_code(&[0xba, 0, 0, 0, 0], 1),
    ]
}

test_cases!(sign_boundaries_preserve_unwritten_bits, boundary_cases());
test_sequences!(
    dividend_preparation_and_pending_flags,
    dividend_and_flag_sequences()
);
