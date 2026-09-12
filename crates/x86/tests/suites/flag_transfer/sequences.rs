use super::{byte_flags, concrete_record, sahf_flags};
use crate::support::{
    cases::{
        FlagExpectation::{Clear, Preserved, Set},
        Flags,
    },
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Case},
};
use wasm86_x86::Gpr32::{Eax, Ebx, Ecx, Esi};

fn local_sources_and_aliases() -> Vec<Case> {
    vec![
        Case::new(
            "LAHF reads pending byte ADD; SAHF preserves its overflow",
            Flags::all(false),
        )
        .initial_registers(&[(Eax, 0x4433_227f), (Ebx, 0x8877_6600), (Ecx, 0)])
        .instruction_count(u32::MAX - 3)
        .step(
            Step::new(
                &[0x04, 1],
                Flags {
                    cf: Clear,
                    pf: Clear,
                    af: Set,
                    zf: Clear,
                    sf: Set,
                    of: Set,
                },
            )
            .register(Eax, 0x4433_2280),
        )
        .step(Step::preserving_flags(&[0x9f]).register(Eax, 0x4433_9280))
        .step(Step::preserving_flags(&[0xb4, 0x6d]).register(Eax, 0x4433_6d80))
        .step(Step::new(&[0x9e], sahf_flags(0x6d)))
        .step(Step::preserving_flags(&[0x0f, 0x90, 0xc3]).register(Ebx, 0x8877_6601))
        .step(Step::preserving_flags(&[0x0f, 0x92, 0xc1]).register(Ecx, 1))
        .step(Step::preserving_flags(&[0x66, 0x66, 0x9f]).register(Eax, 0x4433_4780)),
        Case::new(
            "AH transfers follow AL, AX and EAX writes",
            Flags::all(true),
        )
        .initial_register(Eax, 0x4433_d711)
        .step(Step::new(&[0x9e], sahf_flags(0xd7)))
        .step(Step::preserving_flags(&[0x9f]).register(Eax, 0x4433_d711))
        .step(Step::preserving_flags(&[0xb0, 0xaa]).register(Eax, 0x4433_d7aa))
        .step(Step::preserving_flags(&[0x66, 0xb8, 0xcc, 0x28]).register(Eax, 0x4433_28cc))
        .step(Step::new(&[0x66, 0x9e], sahf_flags(0x28)))
        .step(Step::preserving_flags(&[0x9f]).register(Eax, 0x4433_02cc))
        .step(Step::preserving_flags(&[0xb8, 0x55, 0xff, 0x34, 0x12]).register(Eax, 0x1234_ff55))
        .step(Step::new(&[0x66, 0x66, 0x9e], sahf_flags(0xff)))
        .step(Step::preserving_flags(&[0x9f]).register(Eax, 0x1234_d755)),
        Case::new(
            "LAHF reads preserved carry and INC flags; SAHF feeds a signed condition",
            Flags::all(false),
        )
        .initial_registers(&[(Eax, 0x7fff_ffff), (Ecx, 0)])
        .step(Step::new(
            &[0xf9],
            Flags {
                cf: Set,
                ..Flags::all(Preserved)
            },
        ))
        .step(
            Step::new(
                &[0x40],
                Flags {
                    cf: Preserved,
                    pf: Set,
                    af: Set,
                    zf: Clear,
                    sf: Set,
                    of: Set,
                },
            )
            .register(Eax, 0x8000_0000),
        )
        .step(Step::preserving_flags(&[0x9f]).register(Eax, 0x8000_9700))
        .step(Step::preserving_flags(&[0xb4, 2]).register(Eax, 0x8000_0200))
        .step(Step::new(&[0x9e], sahf_flags(2)))
        .step(Step::preserving_flags(&[0x0f, 0x9e, 0xc1]).register(Ecx, 1)),
        Case::new(
            "SAHF replaces pending subtraction flags before carry arithmetic",
            Flags::all(false),
        )
        .initial_registers(&[(Eax, 0x4433_0111), (Ecx, 0), (Ebx, 0)])
        .step(
            Step::new(
                &[0x83, 0xe9, 1],
                Flags {
                    cf: Set,
                    pf: Set,
                    af: Set,
                    zf: Clear,
                    sf: Set,
                    of: Clear,
                },
            )
            .register(Ecx, u32::MAX),
        )
        .step(Step::new(&[0x9e], sahf_flags(1)))
        .step(
            Step::new(
                &[0x83, 0xd3, 0],
                Flags {
                    cf: Clear,
                    pf: Clear,
                    af: Clear,
                    zf: Clear,
                    sf: Clear,
                    of: Clear,
                },
            )
            .register(Ebx, 1),
        )
        .step(Step::preserving_flags(&[0x9f]).register(Eax, 0x4433_0211)),
    ]
}

fn guest_fault_publication() -> Vec<Case> {
    let flags = byte_flags(0x93, true);
    vec![
        Case::new("LAHF completes before a guest write fault", flags)
            .stored_flags(concrete_record(flags, 0xfe))
            .initial_registers(&[(Eax, 0x4433_0011), (Esi, 0x6000)])
            .step(Step::preserving_flags(&[0x9f]).register(Eax, 0x4433_9311))
            .step(Step::preserving_flags(&[0x89, 0x06]).fault(0x6000, 2))
            .trailing_code(&[0x9e], 1),
        Case::new("SAHF completes before a guest read fault", flags)
            .stored_flags(concrete_record(flags, 0x81))
            .initial_registers(&[(Eax, 0x4433_4611), (Esi, 0x6000)])
            .step(Step::new(&[0x9e], sahf_flags(0x46)))
            .step(Step::preserving_flags(&[0x8b, 0x06]).fault(0x6000, 0))
            .trailing_code(&[0x9f], 1),
        Case::preserving_flags("a guest fault prevents both AH transfers")
            .initial_register(Esi, 0x6000)
            .step(Step::preserving_flags(&[0x8b, 0x06]).fault(0x6000, 0))
            .trailing_code(&[0x9e, 0x9f], 2),
    ]
}

test_sequences!(
    pending_flags_and_mixed_width_register_aliases,
    local_sources_and_aliases()
);
test_sequences!(
    completed_transfers_survive_guest_faults,
    guest_fault_publication()
);
