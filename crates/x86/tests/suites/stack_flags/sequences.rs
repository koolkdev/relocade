use super::{logical_flags, status_expectations, stored_flags};
use crate::flags::Flag;
use crate::support::{
    cases::{
        FlagExpectation::{Preserved, Set},
        Flags,
        Permissions::{ReadOnly, ReadWrite},
    },
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Case},
};
use wasm86_x86::Gpr32::{Eax, Ebx, Ecx, Esi, Esp};

fn popped(code: &[u8], status: u8, direct: u8, word: bool) -> Step {
    let mut step = Step::new(code, status_expectations(status))
        .expect_direct_flag(Flag::TF, direct & 1 != 0)
        .expect_direct_flag(Flag::DF, direct & 2 != 0)
        .expect_direct_flag(Flag::NT, direct & 4 != 0);
    if !word {
        step = step
            .expect_direct_flag(Flag::AC, direct & 8 != 0)
            .expect_direct_flag(Flag::ID, direct & 16 != 0);
    }
    step
}

fn histories() -> Vec<Case> {
    let mut cases = vec![
        Case::new(
            "PUSHFD observes pending INC with preserved carry",
            Flags::all(false),
        )
        .stored_flags(stored_flags(0, 21))
        .initial_registers(&[(Eax, 0x7fff_ffff), (Esp, 0x9004)])
        .instruction_count(u32::MAX - 1)
        .memory(0x9000, &[0xa5; 8], ReadWrite)
        .step(Step::new(
            &[0xf9],
            Flags {
                cf: Set,
                ..Flags::all(Preserved)
            },
        ))
        .step(Step::new(&[0x40], status_expectations(55)).register(Eax, 0x8000_0000))
        .step(
            Step::preserving_flags(&[0x9c])
                .register(Esp, 0x9000)
                .expect_memory(0x9000, &[0x97, 0x4b, 0x20, 0]),
        )
        .step(
            Step::preserving_flags(&[0x5b])
                .register(Ebx, 0x0020_4b97)
                .register(Esp, 0x9004),
        ),
        Case::new(
            "POPFD canonical image and stack write survive a later data fault",
            Flags::all(false),
        )
        .stored_flags(stored_flags(0, 0))
        .initial_registers(&[(Esp, 0x9000), (Esi, 0x6000)])
        .memory(0x9000, &[0xff; 8], ReadWrite)
        .step(popped(&[0x9d], 63, 31, false).register(Esp, 0x9004))
        .step(
            Step::preserving_flags(&[0x9c])
                .register(Esp, 0x9000)
                .expect_memory(0x9000, &[0xd7, 0x4f, 0x24, 0]),
        )
        .step(Step::preserving_flags(&[0x8b, 0x06]).fault(0x6000, 0))
        .trailing_code(&[0xfc, 0x9d], 2),
        Case::new(
            "POPF flags feed LAHF, signed comparison and carry arithmetic",
            Flags::all(false),
        )
        .stored_flags(stored_flags(0, 0))
        .initial_registers(&[(Eax, 0x4433_0011), (Ebx, 0), (Ecx, u32::MAX), (Esp, 0x9000)])
        .memory(0x9000, &[0x83, 0x0e, 0x24, 0], ReadWrite)
        .step(popped(&[0x9d], 49, 26, false).register(Esp, 0x9004))
        .step(Step::preserving_flags(&[0x9f]).register(Eax, 0x4433_8311))
        .step(Step::preserving_flags(&[0x0f, 0x9c, 0xc1]).register(Ecx, 0xffff_ff00))
        .step(Step::new(&[0x83, 0xd3, 0], status_expectations(0)).register(Ebx, 1))
        .step(
            Step::preserving_flags(&[0x9c])
                .register(Esp, 0x9000)
                .expect_memory(0x9000, &[2, 6, 0x24, 0]),
        ),
        Case::preserving_flags("an earlier guest fault prevents both flag-stack transfers")
            .stored_flags(stored_flags(21, 10))
            .initial_registers(&[(Esp, 0x9000), (Esi, 0x6000)])
            .step(Step::preserving_flags(&[0x8b, 0x06]).fault(0x6000, 0))
            .trailing_code(&[0x9d, 0x9c], 2),
    ];
    for (name, upper, direct, pushed) in [
        ("set", 0x24, 24, [0xd7, 0x4f, 0x24, 0]),
        ("clear", 0, 0, [0xd7, 0x4f, 0, 0]),
    ] {
        cases.push(
            Case::new(
                format!("word POPF retains {name} AC/ID from an earlier POPFD"),
                Flags::all(true),
            )
            .stored_flags(stored_flags(63, 31))
            .initial_register(Esp, 0x9000)
            .memory(0x9000, &[2, 2, upper, 0, 0xff, 0xff, 0xa5], ReadWrite)
            .step(popped(&[0x9d], 0, direct, false).register(Esp, 0x9004))
            .step(popped(&[0x66, 0x9d], 63, 7, true).register(Esp, 0x9006))
            .step(
                Step::preserving_flags(&[0x9c])
                    .register(Esp, 0x9002)
                    .expect_memory(0x9002, &pushed),
            ),
        );
    }
    for (code, push, width) in [
        (&[0x9d][..], false, 4),
        (&[0x66, 0x9d][..], false, 2),
        (&[0x9c][..], true, 4),
        (&[0x66, 0x9c][..], true, 2),
    ] {
        cases.push(
            Case::new(
                format!("failed {code:02x?} preserves completed arithmetic and raw direct flags"),
                logical_flags(0),
            )
            .stored_flags(stored_flags(0, 31))
            .initial_registers(&[
                (Eax, 0x4433_227f),
                (Esp, if push { 0x4fff + width } else { 0x4fff }),
            ])
            .map_page(4, 0x8000, if push { ReadWrite } else { ReadOnly })
            .backing(0x8fff, &[0xff])
            .step(Step::new(&[0x04, 1], status_expectations(52)).register(Eax, 0x4433_2280))
            .step(Step::preserving_flags(code).fault(0x5000, if push { 2 } else { 0 }))
            .trailing_code(&[0xfc, 0x9c], 2),
        );
    }
    cases
}

test_sequences!(flag_images_compose_with_arithmetic_and_faults, histories());
