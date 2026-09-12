use super::{carry_result, flag_records};
use crate::support::{
    cases::{
        FlagExpectation::{Clear, Set},
        Flags,
    },
    sequences::{test_sequences, Checkpoint, SequenceCase},
};
use wasm86_x86::{
    CpuState,
    Gpr32::{Eax, Ebx, Ecx, Esi},
};

fn carry_consumers() -> Vec<SequenceCase> {
    vec![
        SequenceCase::new(
            "carry controls replace an ADD recipe before ADC, SBB and branch",
            Flags::all(false),
        )
        .initial_registers(&[(Eax, u32::MAX), (Ecx, 0)])
        .step(
            Checkpoint::new(
                &[0x05, 1, 0, 0, 0],
                Flags {
                    cf: Set,
                    pf: Set,
                    af: Set,
                    zf: Set,
                    sf: Clear,
                    of: Clear,
                },
            )
            .register(Eax, 0),
        )
        .step(Checkpoint::new(&[0xf8], carry_result(false)))
        .step(Checkpoint::new(
            &[0x83, 0xd1, 0],
            Flags {
                cf: Clear,
                pf: Set,
                af: Clear,
                zf: Set,
                sf: Clear,
                of: Clear,
            },
        ))
        .step(Checkpoint::new(&[0xf9], carry_result(true)))
        .step(
            Checkpoint::new(
                &[0x83, 0xd9, 0],
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
        .step(Checkpoint::new(&[0xf5], carry_result(false)))
        .step(Checkpoint::preserving_flags(&[0x0f, 0x92, 0xc0]).register(Eax, 0))
        .step(Checkpoint::preserving_flags(&[0x73, 5]).dispatch(0x1018))
        .trailing_code(&[0xf9, 0xfd], 2),
        SequenceCase::new(
            "CMC preserves overflow from local byte ADD for SETO",
            Flags::all(false),
        )
        .initial_registers(&[(Eax, 0x4433_227f), (Ebx, 0x8877_6600), (Ecx, 0xccbb_aa00)])
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
        .step(Checkpoint::new(&[0xf5], carry_result(true)))
        .step(Checkpoint::preserving_flags(&[0x0f, 0x90, 0xc3]).register(Ebx, 0x8877_6601))
        .step(Checkpoint::preserving_flags(&[0x0f, 0x92, 0xc1]).register(Ecx, 0xccbb_aa01))
        .step(Checkpoint::new(&[0xf5], carry_result(false)))
        .step(Checkpoint::preserving_flags(&[0x0f, 0x93, 0xc1]).register(Ecx, 0xccbb_aa01))
        .step(Checkpoint::preserving_flags(&[0x70, 3]).dispatch(0x1012))
        .trailing_code(&[0xfc, 0xf8], 2),
        SequenceCase::new(
            "CLC preserves zero for SETBE after a local subtraction",
            Flags::all(false),
        )
        .initial_registers(&[(Eax, 0x1234_5678), (Ebx, 0x8877_6600)])
        .step(
            Checkpoint::new(
                &[0x29, 0xc0],
                Flags {
                    cf: Clear,
                    pf: Set,
                    af: Clear,
                    zf: Set,
                    sf: Clear,
                    of: Clear,
                },
            )
            .register(Eax, 0),
        )
        .step(Checkpoint::new(&[0xf9], carry_result(true)))
        .step(Checkpoint::new(&[0xf8], carry_result(false)))
        .step(Checkpoint::preserving_flags(&[0x0f, 0x96, 0xc3]).register(Ebx, 0x8877_6601)),
    ]
}

fn direction_sequences() -> Vec<SequenceCase> {
    let mut cases = Vec::new();
    for (opcode, direction) in [(0xfc, false), (0xfd, true)] {
        cases.push(
            SequenceCase::preserving_flags(format!(
                "repeated DF changes ending in {opcode:02x} survive a read fault"
            ))
            .stored_flags(CpuState::filled(0x5a).flags)
            .instruction_count(u32::MAX - 2)
            .initial_registers(&[(Eax, 0x1234_5678), (Esi, 0x6000)])
            .step(Checkpoint::preserving_flags(&[0xfd]).expect_direction_flag(true))
            .step(Checkpoint::preserving_flags(&[0xfc]).expect_direction_flag(false))
            .step(
                Checkpoint::preserving_flags(&[0x66, 0x66, opcode])
                    .expect_direction_flag(direction),
            )
            .step(Checkpoint::preserving_flags(&[opcode]).expect_direction_flag(direction))
            .step(Checkpoint::preserving_flags(&[0xb0, 0x42]).register(Eax, 0x1234_5642))
            .step(Checkpoint::preserving_flags(&[0x8b, 0x06]).fault(0x6000, 0))
            .trailing_code(&[if direction { 0xfc } else { 0xfd }], 1),
        );
    }
    cases.push(
        SequenceCase::new(
            "DF changes preserve a pending ALU result through carry consumption",
            Flags::all(false),
        )
        .initial_registers(&[(Eax, u32::MAX), (Ecx, 0)])
        .step(
            Checkpoint::new(
                &[0x05, 1, 0, 0, 0],
                Flags {
                    cf: Set,
                    pf: Set,
                    af: Set,
                    zf: Set,
                    sf: Clear,
                    of: Clear,
                },
            )
            .register(Eax, 0),
        )
        .step(Checkpoint::preserving_flags(&[0xfd]).expect_direction_flag(true))
        .step(Checkpoint::preserving_flags(&[0xfc]).expect_direction_flag(false))
        .step(
            Checkpoint::new(
                &[0x83, 0xd1, 0],
                Flags {
                    cf: Clear,
                    pf: Clear,
                    af: Clear,
                    zf: Clear,
                    sf: Clear,
                    of: Clear,
                },
            )
            .register(Ecx, 1),
        )
        .step(Checkpoint::preserving_flags(&[0xfd]).expect_direction_flag(true))
        .step(Checkpoint::new(&[0xf9], carry_result(true)))
        .step(Checkpoint::preserving_flags(&[0xfc]).expect_direction_flag(false)),
    );
    cases
}

fn fault_sequences() -> Vec<SequenceCase> {
    let mut cases = Vec::new();
    for (record_name, record, initial) in flag_records() {
        for (opcode, carry) in [(0xf8, false), (0xf9, true), (0xf5, !initial.cf)] {
            cases.push(
                SequenceCase::new(
                    format!("{opcode:02x}, {record_name}, before guest write fault"),
                    initial,
                )
                .stored_flags(record)
                .initial_register(Esi, 0x6000)
                .step(Checkpoint::new(&[opcode], carry_result(carry)))
                .step(Checkpoint::preserving_flags(&[0xfd]).expect_direction_flag(true))
                .step(Checkpoint::preserving_flags(&[0x89, 0x06]).fault(0x6000, 2))
                .trailing_code(&[0xf5, 0xfc], 2),
            );
        }
    }
    for (name, opcode) in super::ENCODINGS {
        cases.push(
            SequenceCase::preserving_flags(format!("guest fault prevents later {name}"))
                .initial_register(Esi, 0x6000)
                .step(Checkpoint::preserving_flags(&[0x8b, 0x06]).fault(0x6000, 0))
                .trailing_code(&[opcode], 1),
        );
    }
    cases.push(
        SequenceCase::new(
            "carry and direction changes survive divide error",
            Flags::all(false),
        )
        .initial_register(Ecx, 0)
        .step(Checkpoint::new(&[0xf9], carry_result(true)))
        .step(Checkpoint::preserving_flags(&[0xfd]).expect_direction_flag(true))
        .step(Checkpoint::preserving_flags(&[0xf7, 0xf1]).divide_error())
        .trailing_code(&[0xf8, 0xfc], 2),
    );
    cases
}

test_sequences!(
    carry_producers_and_consumers_share_a_block,
    carry_consumers()
);
test_sequences!(
    direction_changes_compose_with_flag_recipes,
    direction_sequences()
);
test_sequences!(
    completed_changes_are_published_at_guest_faults,
    fault_sequences()
);
