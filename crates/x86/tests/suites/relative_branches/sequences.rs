use crate::support::{
    cases::{
        FlagExpectation::{Clear, Set},
        Flags,
    },
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Case},
};
use wasm86_x86::Gpr32::Eax;

#[rustfmt::skip]
fn publication_cases() -> Vec<Case> {
    vec![
        Case::from_opaque_flags("SUB produces zero; JNE falls through")
            .initial_register(Eax, 1)
            .step(Step::new(&[0x83, 0xe8, 1], Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear }).register(Eax, 0))
            .step(Step::preserving_flags(&[0x75, 0xfb]).dispatch(0x1005)),
        Case::from_opaque_flags("SUB produces one; JNE branches back")
            .initial_register(Eax, 2)
            .step(Step::new(&[0x83, 0xe8, 1], Flags { cf: Clear, pf: Clear, af: Clear, zf: Clear, sf: Clear, of: Clear }).register(Eax, 1))
            .step(Step::preserving_flags(&[0x75, 0xfb]).dispatch(0x1000)),
        Case::preserving_flags("memory fault before a terminating branch publishes completed instructions")
            .step(Step::preserving_flags(&[0xb8, 7, 0, 0, 0]).register(Eax, 7))
            .step(Step::preserving_flags(&[0x8b, 0x0d, 0, 0x40, 0, 0]).fault(0x4000, 0))
            .trailing_code(&[0xeb, 0x7f], 1),
    ]
}

test_sequences!(arithmetic_branches_and_publication, publication_cases());
