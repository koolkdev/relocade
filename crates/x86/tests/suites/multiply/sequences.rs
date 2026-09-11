use wasm86_x86::Gpr32::{Eax, Ebx, Ecx, Edx, Esi};

use crate::support::{
    cases::{
        FlagExpectation::{Clear, Set},
        Flags,
    },
    sequences::{test_sequences, Checkpoint, SequenceCase as Case},
};

use super::product_flags;

#[rustfmt::skip]
fn product_sequences() -> Vec<Case> {
    vec![
        Case::from_opaque_flags("MUL old AH produces carry and overflow for conditions and ADC")
            .initial_registers(&[(Eax, 0x4433_8080), (Ebx, 0x10ff_eedd), (Edx, 0x7fff_ffff)])
            .step(Checkpoint::new(&[0xf6, 0xe4], product_flags(Set)).register(Eax, 0x4433_4000))
            .step(Checkpoint::preserving_flags(&[0x0f, 0x92, 0xc3]).register(Ebx, 0x10ff_ee01))
            .step(Checkpoint::preserving_flags(&[0x0f, 0x90, 0xc7]).register(Ebx, 0x10ff_0101))
            .step(Checkpoint::new(&[0x83, 0xd2, 0],
                Flags { cf: Clear, pf: Set, af: Set, zf: Clear, sf: Set, of: Set })
                .register(Edx, 0x8000_0000)),
        Case::from_opaque_flags("signed products feed conditions and a later self multiply")
            .initial_registers(&[(Eax, 0xffff_fffe), (Edx, 3), (Ebx, 0x10ff_eedd)])
            .step(Checkpoint::new(&[0x0f, 0xaf, 0xc2], product_flags(Clear)).register(Eax, 0xffff_fffa))
            .step(Checkpoint::preserving_flags(&[0x0f, 0x92, 0xc3]).register(Ebx, 0x10ff_ee00))
            .step(Checkpoint::preserving_flags(&[0x0f, 0x90, 0xc7]).register(Ebx, 0x10ff_0000))
            .step(Checkpoint::new(&[0x0f, 0xaf, 0xc0], product_flags(Clear)).register(Eax, 0x24)),
        Case::from_opaque_flags("a zero immediate still faults and publishes the preceding ADD")
            .instruction_count(0xffff_fffe)
            .initial_registers(&[(Eax, 0x4433_22ff), (Edx, 0xccbb_aa01), (Esi, 0x5000)])
            .step(Checkpoint::new(&[0x00, 0xd0],
                Flags { cf: Set, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear })
                .register(Eax, 0x4433_2200))
            .step(Checkpoint::preserving_flags(&[0x69, 0x16, 0, 0, 0, 0]).fault(0x5000, 0))
            .trailing_code(&[0xb8, 0, 0, 0, 0], 1),
        Case::from_opaque_flags("a later source fault publishes both halves of the completed product")
            .instruction_count(0xffff_fffe)
            .initial_registers(&[(Eax, 0xffff_ffff), (Edx, 0xccbb_aa99), (Ebx, 3), (Ecx, 0x5000)])
            .step(Checkpoint::new(&[0xf7, 0xe3], product_flags(Set))
                .register(Eax, 0xffff_fffd).register(Edx, 2))
            .step(Checkpoint::preserving_flags(&[0x0f, 0xaf, 0x01]).fault(0x5000, 0))
            .trailing_code(&[0xba, 0, 0, 0, 0], 1),
    ]
}

test_sequences!(
    products_conditions_and_fault_publication,
    product_sequences()
);
