use super::pair;
use crate::support::{
    cases::{
        FlagExpectation::{Clear, Set},
        Flags,
        Permissions::ReadWrite,
    },
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Case},
};
use wasm86_x86::Gpr32::*;

fn sequences() -> Vec<Case> {
    let mut cases = Vec::new();
    for (word, prefix, input, output) in [
        (true, &[0x66][..], 0x1234_7fff, 0x1234_8000),
        (false, &[][..], 0x7fff_ffff, 0x8000_0000),
    ] {
        let code = [prefix, &[0x62, 0x03]].concat();
        cases.push(
            Case::from_opaque_flags(format!(
                "BOUND fault preserves preceding aliases, arithmetic and stores, word={word}"
            ))
            .instruction_count(u32::MAX - 2)
            .initial_registers(&[(Eax, input), (Ebx, 0x1111_1111)])
            .memory(0x4000, &pair(word, -5, 7), ReadWrite)
            .step(Step::preserving_flags(&[0xbb, 0, 0x40, 0, 0]).register(Ebx, 0x4000))
            .step(
                Step::preserving_flags(&[0x66, 0xc7, 0x03, 0xfa, 0xff])
                    .expect_memory(0x4000, &[0xfa, 0xff]),
            )
            .step(
                Step::new(
                    &[prefix, &[0x83, 0xc0, 1]].concat(),
                    Flags {
                        cf: Clear,
                        pf: Set,
                        af: Set,
                        zf: Clear,
                        sf: Set,
                        of: Set,
                    },
                )
                .register(Eax, output),
            )
            .step(Step::preserving_flags(&code).bound_range_exceeded())
            .trailing_code(&[0x89, 0xc1], 1),
        );
        cases.push(
            Case::preserving_flags(format!(
                "BOUND succeeds then faults on changed upper bound, word={word}"
            ))
            .initial_registers(&[(Eax, 7), (Ebx, 0x4000)])
            .memory(0x4000, &pair(word, -5, 7), ReadWrite)
            .step(Step::preserving_flags(&code))
            .step(
                Step::preserving_flags(if word {
                    &[0x66, 0xc7, 0x43, 2, 6, 0][..]
                } else {
                    &[0xc7, 0x43, 4, 6, 0, 0, 0][..]
                })
                .expect_memory(
                    if word { 0x4002 } else { 0x4004 },
                    if word { &[6, 0][..] } else { &[6, 0, 0, 0][..] },
                ),
            )
            .step(Step::preserving_flags(&code).bound_range_exceeded())
            .trailing_code(&[0x89, 0xc1], 1),
        );
    }
    cases
}

test_sequences!(continuation_and_fault_publication, sequences());
