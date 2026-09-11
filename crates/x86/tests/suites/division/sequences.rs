use wasm86_x86::Gpr32::{Eax, Ebx, Ecx, Edx, Esi};

use crate::support::{
    cases::{
        FlagExpectation::{Clear, Set, Undefined},
        Flags,
        Permissions::ReadWrite,
    },
    sequences::{test_sequences, Checkpoint, SequenceCase as Case},
};

#[rustfmt::skip]
fn division_sequences() -> Vec<Case> {
    vec![
        Case::new("consecutive divisions consume both prior results before a later overflow", Flags::all(true))
            .instruction_count(0xffff_fffe)
            .initial_registers(&[(Eax, 1), (Edx, 1), (Ebx, 3)])
            .step(Checkpoint::new(&[0xf7, 0xf3], Flags::all(Undefined))
                .register(Eax, 0x5555_5555).register(Edx, 2))
            .step(Checkpoint::new(&[0xf7, 0xf0], Flags::all(Undefined)).register(Eax, 7).register(Edx, 2))
            .step(Checkpoint::preserving_flags(&[0xf7, 0xf2]).divide_error())
            .trailing_code(&[0xb8, 0, 0, 0, 0], 1),
        Case::from_opaque_flags("divide error publishes the preceding ADD and memory store")
            .instruction_count(0xffff_fffe)
            .initial_registers(&[(Eax, 0x1234_5678), (Edx, 0x9876_5432), (Ebx, 0x7fff_ffff), (Ecx, 0), (Esi, 0x4000)])
            .memory(0x4000, &[0xa5, 0xa5, 0xa5, 0xa5], ReadWrite)
            .step(Checkpoint::new(&[0x83, 0xc3, 1],
                Flags { cf: Clear, pf: Set, af: Set, zf: Clear, sf: Set, of: Set }).register(Ebx, 0x8000_0000))
            .step(Checkpoint::preserving_flags(&[0x89, 0x1e]).expect_memory(0x4000, &[0, 0, 0, 0x80]))
            .step(Checkpoint::preserving_flags(&[0xf7, 0xf9]).divide_error())
            .trailing_code(&[0xb8, 0, 0, 0, 0], 1),
        Case::from_opaque_flags("minimum signed double-width overflow publishes prior arithmetic")
            .initial_registers(&[(Eax, 0), (Edx, 0x8000_0000), (Ebx, 0xffff_ffff), (Ecx, 0x7fff_ffff)])
            .step(Checkpoint::new(&[0x83, 0xc1, 1],
                Flags { cf: Clear, pf: Set, af: Set, zf: Clear, sf: Set, of: Set }).register(Ecx, 0x8000_0000))
            .step(Checkpoint::preserving_flags(&[0xf7, 0xfb]).divide_error())
            .trailing_code(&[0xba, 0, 0, 0, 0], 1),
        Case::new("a source fault publishes the preceding signed quotient and remainder", Flags::all(false))
            .initial_registers(&[(Eax, 0xffff_ff9c), (Edx, 0xffff_ffff), (Ebx, 7), (Esi, 0x5000)])
            .step(Checkpoint::new(&[0xf7, 0xfb], Flags::all(Undefined))
                .register(Eax, 0xffff_fff2).register(Edx, 0xffff_fffe))
            .step(Checkpoint::preserving_flags(&[0xf7, 0x36]).fault(0x5000, 0))
            .trailing_code(&[0xb8, 0, 0, 0, 0], 1),
        Case::preserving_flags("constant zero dividend and divisor raise divide error after three MOVs")
            .instruction_count(0xffff_fffe)
            .step(Checkpoint::preserving_flags(&[0xb8, 0, 0, 0, 0]).register(Eax, 0))
            .step(Checkpoint::preserving_flags(&[0xba, 0, 0, 0, 0]).register(Edx, 0))
            .step(Checkpoint::preserving_flags(&[0xb9, 0, 0, 0, 0]).register(Ecx, 0))
            .step(Checkpoint::preserving_flags(&[0xf7, 0xf1]).divide_error())
            .trailing_code(&[0xb8, 1, 0, 0, 0], 1),
        Case::preserving_flags("constant signed double-width minimum over negative one preserves the MOV results")
            .instruction_count(0xffff_fffd)
            .step(Checkpoint::preserving_flags(&[0xb8, 0, 0, 0, 0]).register(Eax, 0))
            .step(Checkpoint::preserving_flags(&[0xba, 0, 0, 0, 0x80]).register(Edx, 0x8000_0000))
            .step(Checkpoint::preserving_flags(&[0xb9, 0xff, 0xff, 0xff, 0xff]).register(Ecx, 0xffff_ffff))
            .step(Checkpoint::preserving_flags(&[0xf7, 0xf9]).divide_error())
            .trailing_code(&[0xba, 0, 0, 0, 0], 1),
    ]
}

#[rustfmt::skip]
fn software_flag_policy() -> Vec<Case> {
    vec![
        Case::from_opaque_flags("division software policy preserves a pending ADD recipe for later conditions")
            .initial_registers(&[(Eax, 0x4433_0101), (Ebx, 3), (Ecx, 0x7fff_ffff), (Edx, 0)])
            .step(Checkpoint::new(&[0x83, 0xc1, 1],
                Flags { cf: Clear, pf: Set, af: Set, zf: Clear, sf: Set, of: Set }).register(Ecx, 0x8000_0000))
            .step(Checkpoint::preserving_flags(&[0xf6, 0xf3]).register(Eax, 0x4433_0255))
            .step(Checkpoint::preserving_flags(&[0x0f, 0x90, 0xc2]).register(Edx, 1))
            .step(Checkpoint::preserving_flags(&[0x0f, 0x9a, 0xc6]).register(Edx, 0x101)),
    ]
}

test_sequences!(
    quotients_remainders_and_fault_publication,
    division_sequences()
);
test_sequences!(
    undefined_flags_follow_software_policy_across_instructions,
    software_flag_policy()
);
