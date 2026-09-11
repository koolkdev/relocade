use crate::support::cases::Permissions::ReadWrite;
use crate::support::sequences::{test_sequences, Checkpoint as Step, SequenceCase as Case};
use wasm86_x86::Gpr32::*;

#[rustfmt::skip]
fn register_sequences() -> Vec<Case> {
    vec![
        Case::preserving_flags("interleaved full and partial definitions")
            .initial_registers(&[(Eax, 0x4433_2211), (Ecx, 0x8877_6655)])
            .step(Step::preserving_flags(&[0xb8, 0x78, 0x56, 0x34, 0x12]).register(Eax, 0x1234_5678))
            .step(Step::preserving_flags(&[0xb4, 0xab]).register(Eax, 0x1234_ab78))
            .step(Step::preserving_flags(&[0xb0, 0xcd]).register(Eax, 0x1234_abcd))
            .step(Step::preserving_flags(&[0x8b, 0xc8]).register(Ecx, 0x1234_abcd))
            .step(Step::preserving_flags(&[0xb8, 0x98, 0xba, 0xdc, 0xfe]).register(Eax, 0xfedc_ba98)),
        Case::preserving_flags("old high byte survives later alias writes")
            .initial_registers(&[(Eax, 0x4433_2211), (Ecx, 0x8877_6655)])
            .step(Step::preserving_flags(&[0x8a, 0xcc]).register(Ecx, 0x8877_6622))
            .step(Step::preserving_flags(&[0xb4, 0xff]).register(Eax, 0x4433_ff11))
            .step(Step::preserving_flags(&[0x88, 0xc4]).register(Eax, 0x4433_1111)),
    ]
}

#[rustfmt::skip]
fn memory_sequences() -> Vec<Case> {
    vec![
        Case::preserving_flags("partial definition feeds address and full-register source")
            .initial_registers(&[(Eax, 0x4433_2211), (Ecx, 0x5000), (Ebx, 0x4000)])
            .map_page(4, 0x8000, ReadWrite).map_page(5, 0x9000, ReadWrite)
            .backing(0x801f, &[0xa5, 0x80, 0x5a]).backing(0x8fff, &[0xa5, 0, 0, 0, 0, 0x5a])
            .step(Step::preserving_flags(&[0xb3, 0x20]).register(Ebx, 0x4020))
            .step(Step::preserving_flags(&[0x8a, 0x23]).register(Eax, 0x4433_8011))
            .step(Step::preserving_flags(&[0x89, 0x01]).expect_memory(0x5000, &[0x11, 0x80, 0x33, 0x44])),
        Case::preserving_flags("data fault publishes only the completed byte definition")
            .initial_registers(&[(Eax, 0x4433_2211), (Ecx, 0x5000), (Ebx, 0x4000)])
            .backing(0x801f, &[0xa5, 0x80, 0x5a]).backing(0x8fff, &[0xa5, 0, 0, 0, 0, 0x5a])
            .step(Step::preserving_flags(&[0xb3, 0x20]).register(Ebx, 0x4020))
            .step(Step::preserving_flags(&[0x8a, 0x23]).fault(0x4020, 0))
            .trailing_code(&[0x89, 0x01], 1),
        Case::preserving_flags("byte snapshot survives an aliased guest store")
            .initial_registers(&[(Eax, 0x4433_2211), (Ecx, 0x6000), (Edx, 0xccbb_aa99), (Ebx, 0x4000)])
            .map_page(4, 0x8000, ReadWrite).map_page(6, 0x8000, ReadWrite)
            .backing(0x7fff, &[0xa5, 0x80, 0x5a])
            .step(Step::preserving_flags(&[0x8a, 0x23]).register(Eax, 0x4433_8011))
            .step(Step::preserving_flags(&[0x88, 0x11]).expect_memory(0x6000, &[0x99]))
            .step(Step::preserving_flags(&[0x8a, 0xf4]).register(Edx, 0xccbb_8099)),
    ]
}

test_sequences!(mixed_register_views, register_sequences());
test_sequences!(memory_aliases_and_publication, memory_sequences());
