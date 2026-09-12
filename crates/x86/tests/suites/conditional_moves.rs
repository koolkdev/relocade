use wasm86_x86::StoredStatusSource;
use wasm86_x86::{CpuState, Gpr32, StoredFlags};

use crate::support::{
    cases::{test_cases, FlagExpectation::Preserved, Flags, InstructionCase as Case},
    sequences::{test_sequences, Checkpoint, SequenceCase},
};

#[path = "conditional_moves/decoding.rs"]
mod decoding;
#[path = "conditional_moves/memory.rs"]
mod memory;

const INITIAL_FLAGS: Flags<bool> = Flags::all(true);
const PRESERVED_FLAGS: Flags<crate::support::cases::FlagExpectation> = Flags::all(Preserved);

fn register_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    // Each array gives literal O, NO, B, AE, Z, NZ, BE, A, S, NS, P, NP,
    // L, GE, LE, G outcomes. Both outcomes occur for every condition.
    for (name, flags, taken) in [
        (
            "condition inputs clear",
            Flags {
                cf: false,
                pf: false,
                af: true,
                zf: false,
                sf: false,
                of: false,
            },
            [
                false, true, false, true, false, true, false, true, false, true, false, true,
                false, true, false, true,
            ],
        ),
        (
            "carry and overflow",
            Flags {
                cf: true,
                pf: true,
                af: true,
                zf: true,
                sf: false,
                of: true,
            },
            [
                true, false, true, false, true, false, true, false, false, true, true, false, true,
                false, true, false,
            ],
        ),
        (
            "negative without overflow",
            Flags {
                cf: false,
                pf: false,
                af: true,
                zf: false,
                sf: true,
                of: false,
            },
            [
                false, true, false, true, false, true, false, true, true, false, false, true, true,
                false, true, false,
            ],
        ),
    ] {
        for (prefix, replacement) in [(&[][..], 0x8877_6655), (&[0x66][..], 0x4433_6655)] {
            for (condition, taken) in taken.into_iter().enumerate() {
                let code = [prefix, &[0x0f, 0x40 + condition as u8, 0xc1]].concat();
                cases.push(
                    Case::new(
                        format!("{name}, prefix {prefix:02x?}, condition {condition:x}"),
                        &code,
                        flags,
                        PRESERVED_FLAGS,
                    )
                    .preserve_flag_record()
                    .register(
                        Gpr32::Eax,
                        0x4433_2211,
                        if taken { replacement } else { 0x4433_2211 },
                    )
                    .initial_register(Gpr32::Ecx, 0x8877_6655),
                );
            }
        }
    }
    for (code, destination, value) in [
        (&[0x66, 0x0f, 0x44, 0xe5][..], Gpr32::Esp, 0x5555_6666),
        (&[0x0f, 0x44, 0xee][..], Gpr32::Ebp, 0x7777_7777),
        (&[0x66, 0x0f, 0x44, 0xf7][..], Gpr32::Esi, 0x7777_8888),
        (&[0x0f, 0x44, 0xfc][..], Gpr32::Edi, 0x5555_5555),
    ] {
        cases.push(
            Case::new(
                format!("upper register code {code:02x?}"),
                code,
                INITIAL_FLAGS,
                PRESERVED_FLAGS,
            )
            .preserve_flag_record()
            .expect_register(
                destination,
                crate::support::cases::RegisterExpectation::Exact(value),
            ),
        );
    }
    cases
}

test_cases!(conditions_and_register_widths, register_cases());

fn stored_flag_cases() -> Vec<Case> {
    [
        (
            "byte ADD overflow",
            2,
            0x80,
            0x80,
            0x40,
            0x8877_6655,
            Flags {
                cf: true,
                pf: true,
                af: false,
                zf: true,
                sf: false,
                of: true,
            },
        ),
        (
            "word SUB signed comparison",
            5,
            0x7ffe,
            0xfffe,
            0x4c,
            0x4433_2211,
            Flags {
                cf: true,
                pf: true,
                af: false,
                zf: false,
                sf: true,
                of: true,
            },
        ),
        (
            "dword logical zero",
            11,
            0,
            0x1234_5678,
            0x44,
            0x8877_6655,
            Flags {
                cf: false,
                pf: true,
                af: false,
                zf: true,
                sf: false,
                of: false,
            },
        ),
    ]
    .into_iter()
    .map(|(name, kind, left, right, opcode, eax, flags)| {
        Case::new(name, &[0x0f, opcode, 0xc1], flags, PRESERVED_FLAGS)
            .stored_flags(StoredFlags {
                status_source: StoredStatusSource {
                    kind,
                    left,
                    right,
                    ..(CpuState::filled(0xa5).flags).status_source
                },
                ..CpuState::filled(0xa5).flags
            })
            .preserve_flag_record()
            .register(Gpr32::Eax, 0x4433_2211, eax)
            .initial_register(Gpr32::Ecx, 0x8877_6655)
    })
    .collect()
}

test_cases!(
    stored_flags_are_read_without_materialization,
    stored_flag_cases()
);

test_sequences!(
    self_sources_and_mixed_aliases,
    [
        SequenceCase::new("conditional mixed aliases", INITIAL_FLAGS)
            .initial_register(Gpr32::Eax, 0x4433_2211)
            .initial_register(Gpr32::Edx, 0xdead_c0de)
            .step(Checkpoint::preserving_flags(&[0x66, 0x0f, 0x44, 0xc0]))
            .step(Checkpoint::preserving_flags(&[0xb4, 0x80]).register(Gpr32::Eax, 0x4433_8011))
            .step(
                Checkpoint::preserving_flags(&[0x66, 0x0f, 0x44, 0xc2])
                    .register(Gpr32::Eax, 0x4433_c0de)
            )
            .step(Checkpoint::preserving_flags(&[0x0f, 0x45, 0xc1]))
            .step(Checkpoint::preserving_flags(&[0x0f, 0xb6, 0xcc]).register(Gpr32::Ecx, 0xc0))
            .step(Checkpoint::preserving_flags(&[0x0f, 0x44, 0xc1]).register(Gpr32::Eax, 0xc0))
    ]
);
