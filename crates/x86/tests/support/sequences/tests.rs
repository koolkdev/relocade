use wasm86_x86::Gpr32::{Eax, Ebx, Edx, Esi};

use super::{test_sequences, Checkpoint, SequenceCase};
use crate::support::cases::{
    FlagExpectation::{Clear, Set, Undefined},
    Flags,
    Permissions::ReadWrite,
    RegisterExpectation::DefinedBits,
};

fn checkpoint_cases() -> Vec<SequenceCase> {
    vec![
        SequenceCase::new(
            "ADD flags survive EIP wrap and a partial write, then feed conditions",
            Flags::all(true),
        )
        .at(0xffff_fffe)
        .initial_register(Eax, 0x4433_227f)
        .step(
            Checkpoint::new(
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
        .step(Checkpoint::preserving_flags(&[0xb4, 0x12]).register(Eax, 0x4433_1280))
        .conditions([1, 0, 0, 1, 0, 1, 0, 1, 1, 0, 0, 1, 0, 1, 0, 1]),
        SequenceCase::preserving_flags("overlapping writes and retirement survive a later fault")
            .instruction_count(u32::MAX - 1)
            .initial_registers(&[(Eax, 0x7a), (Ebx, 0x4000), (Edx, 0x1234), (Esi, 0x6000)])
            .map_page(4, 0x8000, ReadWrite)
            .backing(0x7fff, &[0xa5, 0, 0, 0x5a])
            .step(Checkpoint::preserving_flags(&[0x88, 0x03]).expect_memory(0x4000, &[0x7a]))
            .step(
                Checkpoint::preserving_flags(&[0x66, 0x89, 0x13])
                    .expect_memory(0x4000, &[0x34, 0x12]),
            )
            .step(Checkpoint::preserving_flags(&[0x89, 0x0e]).fault(0x6000, 2))
            .trailing_code(&[0xb8, 7, 0, 0, 0], 1),
        SequenceCase::from_opaque_flags(
            "defined register bits are restored by later literal writes",
        )
        .initial_registers(&[(Eax, 0x4433_2281), (Edx, 1)])
        .step(
            Checkpoint::new(&[0x66, 0x0f, 0xa4, 0xd0, 17], Flags::all(Undefined)).expect_register(
                Eax,
                DefinedBits {
                    value: 0x4433_0000,
                    mask: 0xffff_0000,
                },
            ),
        )
        .step(Checkpoint::preserving_flags(&[0xb4, 0x7e]).expect_register(
            Eax,
            DefinedBits {
                value: 0x4433_7e00,
                mask: 0xffff_ff00,
            },
        ))
        .step(Checkpoint::preserving_flags(&[0xb0, 0x42]).register(Eax, 0x4433_7e42)),
        SequenceCase::from_opaque_flags("later byte stores define an earlier undefined word")
            .initial_registers(&[(Eax, 1), (Ebx, 0x4000)])
            .memory(0x3fff, &[0xa5, 0x81, 0x80, 0x5a], ReadWrite)
            .step(
                Checkpoint::new(&[0x66, 0x0f, 0xa4, 0x03, 17], Flags::all(Undefined))
                    .undefined_memory(0x4000, 2),
            )
            .step(Checkpoint::preserving_flags(&[0xc6, 0x03, 7]).expect_memory(0x4000, &[7]))
            .step(
                Checkpoint::preserving_flags(&[0xc6, 0x43, 1, 0x23]).expect_memory(0x4001, &[0x23]),
            ),
    ]
}

test_sequences!(literal_checkpoints, checkpoint_cases());
