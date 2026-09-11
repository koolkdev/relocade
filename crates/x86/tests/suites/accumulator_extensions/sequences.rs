use crate::support::{
    cases::{
        FlagExpectation::{Clear, Set, Undefined},
        Flags,
    },
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Case},
};
use wasm86_x86::Gpr32::{Eax, Ebx, Ecx, Edx};

#[rustfmt::skip]
fn alias_and_flag_sequences() -> Vec<Case> {
    vec![
        Case::preserving_flags("extensions consume the current byte, word and dword aliases")
            .initial_registers(&[(Eax, 0x4433_0000), (Edx, 0xccbb_1234)])
            .step(Step::preserving_flags(&[0xb0, 0x80]).register(Eax, 0x4433_0080))
            .step(Step::preserving_flags(&[0x66, 0x98]).register(Eax, 0x4433_ff80))
            .step(Step::preserving_flags(&[0xb4, 0]).register(Eax, 0x4433_0080))
            .step(Step::preserving_flags(&[0x98]).register(Eax, 0x0000_0080))
            .step(Step::preserving_flags(&[0x66, 0x99]).register(Edx, 0xccbb_0000))
            .step(Step::preserving_flags(&[0xb8, 1, 0, 0, 0x80]).register(Eax, 0x8000_0001))
            .step(Step::preserving_flags(&[0x99]).register(Edx, 0xffff_ffff))
            .step(Step::preserving_flags(&[0x66, 0xb8, 0xff, 0x7f]).register(Eax, 0x8000_7fff))
            .step(Step::preserving_flags(&[0x66, 0x99]).register(Edx, 0xffff_0000))
            .step(Step::preserving_flags(&[0x98]).register(Eax, 0x0000_7fff))
            .step(Step::preserving_flags(&[0x99]).register(Edx, 0)),
        Case::from_opaque_flags("all four extensions preserve pending arithmetic for later conditions")
            .initial_registers(&[(Eax, 0x4433_7f80), (Edx, 0xccbb_0000), (Ebx, 0x7fff_ffff), (Ecx, 0)])
            .step(Step::new(&[0x83, 0xc3, 1],
                Flags { cf: Clear, pf: Set, af: Set, zf: Clear, sf: Set, of: Set }).register(Ebx, 0x8000_0000))
            .step(Step::preserving_flags(&[0x66, 0x98]).register(Eax, 0x4433_ff80))
            .step(Step::preserving_flags(&[0x98]).register(Eax, 0xffff_ff80))
            .step(Step::preserving_flags(&[0x66, 0x99]).register(Edx, 0xccbb_ffff))
            .step(Step::preserving_flags(&[0x99]).register(Edx, 0xffff_ffff))
            .step(Step::preserving_flags(&[0x0f, 0x90, 0xc1]).register(Ecx, 1))
            .step(Step::preserving_flags(&[0x0f, 0x92, 0xc5]).register(Ecx, 1)),
    ]
}

#[rustfmt::skip]
fn dividend_sequences() -> Vec<Case> {
    vec![
        Case::new("CBW prepares a negative byte dividend after MOV AL", Flags::all(true))
            .instruction_count(0xffff_fffe)
            .initial_registers(&[(Eax, 0x4433_2211), (Ecx, 7)])
            .step(Step::preserving_flags(&[0xb0, 0x9c]).register(Eax, 0x4433_229c))
            .step(Step::preserving_flags(&[0x66, 0x98]).register(Eax, 0x4433_ff9c))
            .step(Step::new(&[0xf6, 0xf9], Flags::all(Undefined)).register(Eax, 0x4433_fef2)),
        Case::new("CWD prepares DX while preserving EAX and the high EDX half", Flags::all(false))
            .instruction_count(0xffff_fffe)
            .initial_registers(&[(Eax, 0x4433_2211), (Edx, 0xccbb_aa99), (Ecx, 7)])
            .step(Step::preserving_flags(&[0x66, 0xb8, 0x9c, 0xff]).register(Eax, 0x4433_ff9c))
            .step(Step::preserving_flags(&[0x66, 0x99]).register(Edx, 0xccbb_ffff))
            .step(Step::new(&[0x66, 0xf7, 0xf9], Flags::all(Undefined))
                .register(Eax, 0x4433_fff2).register(Edx, 0xccbb_fffe)),
        Case::new("CWDE and CDQ prepare a dword dividend from a preceding AX write", Flags::all(true))
            .instruction_count(0xffff_fffd)
            .initial_registers(&[(Eax, 0x4433_2211), (Edx, 0xccbb_aa99), (Ecx, 7)])
            .step(Step::preserving_flags(&[0x66, 0xb8, 0x9c, 0xff]).register(Eax, 0x4433_ff9c))
            .step(Step::preserving_flags(&[0x98]).register(Eax, 0xffff_ff9c))
            .step(Step::preserving_flags(&[0x99]).register(Edx, 0xffff_ffff))
            .step(Step::new(&[0xf7, 0xf9], Flags::all(Undefined))
                .register(Eax, 0xffff_fff2).register(Edx, 0xffff_fffe)),
        Case::preserving_flags("byte quotient overflow publishes the preceding MOV and CBW")
            .instruction_count(0xffff_fffe)
            .initial_registers(&[(Eax, 0x4433_2211), (Ecx, 0xff)])
            .step(Step::preserving_flags(&[0xb0, 0x80]).register(Eax, 0x4433_2280))
            .step(Step::preserving_flags(&[0x66, 0x98]).register(Eax, 0x4433_ff80))
            .step(Step::preserving_flags(&[0xf6, 0xf9]).divide_error())
            .trailing_code(&[0xb8, 0, 0, 0, 0], 1),
        Case::preserving_flags("word quotient overflow publishes the preceding MOV and CWD")
            .instruction_count(0xffff_fffe)
            .initial_registers(&[(Eax, 0x4433_2211), (Edx, 0xccbb_aa99), (Ecx, 0xffff)])
            .step(Step::preserving_flags(&[0x66, 0xb8, 0, 0x80]).register(Eax, 0x4433_8000))
            .step(Step::preserving_flags(&[0x66, 0x99]).register(Edx, 0xccbb_ffff))
            .step(Step::preserving_flags(&[0x66, 0xf7, 0xf9]).divide_error())
            .trailing_code(&[0xba, 0, 0, 0, 0], 1),
        Case::preserving_flags("dword quotient overflow publishes the preceding MOV and CDQ")
            .instruction_count(0xffff_fffe)
            .initial_registers(&[(Eax, 0x4433_2211), (Edx, 0xccbb_aa99), (Ecx, 0xffff_ffff)])
            .step(Step::preserving_flags(&[0xb8, 0, 0, 0, 0x80]).register(Eax, 0x8000_0000))
            .step(Step::preserving_flags(&[0x99]).register(Edx, 0xffff_ffff))
            .step(Step::preserving_flags(&[0xf7, 0xf9]).divide_error())
            .trailing_code(&[0xba, 0, 0, 0, 0], 1),
    ]
}

test_sequences!(aliases_and_pending_flags, alias_and_flag_sequences());
test_sequences!(
    signed_dividend_preparation_and_fault_publication,
    dividend_sequences()
);
