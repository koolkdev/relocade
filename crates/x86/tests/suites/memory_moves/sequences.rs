use crate::support::{
    cases::Permissions::{ReadOnly, ReadWrite},
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Case},
};
use wasm86_x86::Gpr32::*;

#[rustfmt::skip]
fn progress_cases() -> Vec<Case> {
    vec![
        Case::preserving_flags("successful memory load continues")
            .initial_register(Ebx, 0x4000).memory(0x4000, &[0x78, 0x56, 0x34, 0x92], ReadWrite)
            .step(Step::preserving_flags(&[0xb8, 42, 0, 0, 0]).register(Eax, 42))
            .step(Step::preserving_flags(&[0x8b, 0x13]).register(Edx, 0x9234_5678))
            .step(Step::preserving_flags(&[0xb9, 7, 0, 0, 0]).register(Ecx, 7)),
        Case::preserving_flags("successful memory store continues")
            .initial_register(Ebx, 0x4000).memory(0x4000, &[0x78, 0x56, 0x34, 0x92], ReadWrite)
            .step(Step::preserving_flags(&[0xb8, 42, 0, 0, 0]).register(Eax, 42))
            .step(Step::preserving_flags(&[0x89, 0x03]).expect_memory(0x4000, &[42, 0, 0, 0]))
            .step(Step::preserving_flags(&[0xb9, 7, 0, 0, 0]).register(Ecx, 7)),
        Case::preserving_flags("read fault publishes only completed prefix")
            .initial_register(Ebx, 0x4000).backing(0x8000, &[0x78, 0x56, 0x34, 0x92])
            .step(Step::preserving_flags(&[0xb8, 42, 0, 0, 0]).register(Eax, 42))
            .step(Step::preserving_flags(&[0x8b, 0x13]).fault(0x4000, 0)).trailing_code(&[0xb9, 7, 0, 0, 0], 1),
        Case::preserving_flags("write fault publishes only completed prefix")
            .initial_register(Ebx, 0x4000).memory(0x4000, &[0x78, 0x56, 0x34, 0x92], ReadOnly)
            .step(Step::preserving_flags(&[0xb8, 42, 0, 0, 0]).register(Eax, 42))
            .step(Step::preserving_flags(&[0x89, 0x03]).fault(0x4000, 3)).trailing_code(&[0xb9, 7, 0, 0, 0], 1),
        Case::preserving_flags("completed store survives a later read fault")
            .initial_registers(&[(Ebx, 0x4000), (Ecx, 0x5000)]).memory(0x4000, &[0xa5; 4], ReadWrite)
            .step(Step::preserving_flags(&[0xb8, 42, 0, 0, 0]).register(Eax, 42))
            .step(Step::preserving_flags(&[0x89, 0x03]).expect_memory(0x4000, &[42, 0, 0, 0]))
            .step(Step::preserving_flags(&[0x8b, 0x11]).fault(0x5000, 0)),
        Case::preserving_flags("scattered store completes before a later read faults")
            .initial_registers(&[(Ebx, 0x4ffe), (Ecx, 0x6000)])
            .map_page(4, 0x8000, ReadWrite).map_page(5, 0xa000, ReadWrite).memory(0x4ffd, &[0xa5, 0x78, 0x56, 0x34, 0x92, 0x5a], ReadWrite)
            .step(Step::preserving_flags(&[0xb8, 42, 0, 0, 0]).register(Eax, 42))
            .step(Step::preserving_flags(&[0x89, 0x03]).expect_memory(0x4ffe, &[42, 0, 0, 0]))
            .step(Step::preserving_flags(&[0x8b, 0x11]).fault(0x6000, 0)),
    ]
}

#[rustfmt::skip]
fn alias_cases() -> Vec<Case> {
    vec![
        Case::preserving_flags("memory address uses the preceding register definition")
            .memory(0x4000, &[0x78, 0x56, 0x34, 0x92], ReadWrite)
            .step(Step::preserving_flags(&[0xbb, 0, 0x40, 0, 0]).register(Ebx, 0x4000))
            .step(Step::preserving_flags(&[0x8b, 0x03]).register(Eax, 0x9234_5678))
            .step(Step::preserving_flags(&[0xbe, 7, 0, 0, 0]).register(Esi, 7)),
        Case::preserving_flags("data fault publishes the preceding address definition")
            .backing(0x8000, &[0x78, 0x56, 0x34, 0x92])
            .step(Step::preserving_flags(&[0xbb, 0, 0x40, 0, 0]).register(Ebx, 0x4000))
            .step(Step::preserving_flags(&[0x8b, 0x03]).fault(0x4000, 0)).trailing_code(&[0xbe, 7, 0, 0, 0], 1),
        Case::preserving_flags("distinct guest pages alias the loaded snapshot")
            .initial_registers(&[(Ebx, 0x4000), (Ecx, 0x6000), (Edx, 0xdead_beef)])
            .map_page(4, 0x8000, ReadWrite).map_page(6, 0x8000, ReadWrite).backing(0x7fff, &[0xa5, 0x78, 0x56, 0x34, 0x92, 0x5a])
            .step(Step::preserving_flags(&[0x8b, 0x03]).register(Eax, 0x9234_5678))
            .step(Step::preserving_flags(&[0x89, 0x11]).expect_memory(0x6000, &[0xef, 0xbe, 0xad, 0xde]))
            .step(Step::preserving_flags(&[0x8b, 0xf0]).register(Esi, 0x9234_5678)),
    ]
}

test_sequences!(progress_and_faults, progress_cases());
test_sequences!(forwarded_addresses_and_aliases, alias_cases());
