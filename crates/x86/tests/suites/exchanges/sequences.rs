use super::LAZY_FLAGS;
use crate::support::{
    cases::Permissions::ReadWrite,
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Case},
};
use wasm86_x86::Gpr32::*;

#[rustfmt::skip]
fn sequences() -> Vec<Case> {
    vec![Case::preserving_flags("sequential register and memory aliases publish before a later fault")
        .stored_flags(LAZY_FLAGS).instruction_count(0xffff_fffd)
        .initial_registers(&[(Eax, 0x4433_2211), (Ecx, 0x6000), (Edx, 0xccbb_aa99), (Ebx, 0x4000)])
        .map_page(4, 0x8000, ReadWrite).map_page(6, 0x8000, ReadWrite)
        .backing(0x7fff, &[0x5a, 0x10, 0x40, 0, 0, 0x5a]).backing(0x800f, &[0x5a, 0x80, 0x5a])
        .step(Step::preserving_flags(&[0x86, 0xc4]).register(Eax, 0x4433_1122))
        .step(Step::preserving_flags(&[0x66, 0x92]).register(Eax, 0x4433_aa99).register(Edx, 0xccbb_1122))
        .step(Step::preserving_flags(&[0x87, 0x03]).register(Eax, 0x0000_4010).expect_memory(0x4000, &[0x99, 0xaa, 0x33, 0x44]))
        .step(Step::preserving_flags(&[0x86, 0x20]).register(Eax, 0x0000_8010).expect_memory(0x4010, &[0x40]))
        .step(Step::preserving_flags(&[0x66, 0x87, 0x11]).register(Edx, 0xccbb_aa99).expect_memory(0x6000, &[0x22, 0x11]))
        .step(Step::preserving_flags(&[0x87, 0x00]).fault(0x8010, 2))]
}

test_sequences!(aliases_and_publication, sequences());
