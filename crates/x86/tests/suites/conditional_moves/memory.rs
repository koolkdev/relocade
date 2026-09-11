use wasm86_x86::Gpr32;

use super::{INITIAL_FLAGS, PRESERVED_FLAGS};
use crate::support::{
    cases::{
        test_cases,
        FlagExpectation::{Clear, Set, Undefined},
        Flags, InstructionCase as Case,
        Permissions::ReadOnly,
    },
    sequences::{test_sequences, Checkpoint, SequenceCase},
};

fn source_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (code, address, replacement) in [
        (&[0x66, 0x0f, 0x44, 0x03][..], 0x4ffe, 0x4433_1234),
        (&[0x0f, 0x44, 0x03][..], 0x4ffc, 0x1234_88a1),
    ] {
        for zero in [false, true] {
            cases.push(
                Case::new(
                    format!("readonly source at {address:x}, ZF={zero}"),
                    code,
                    Flags {
                        zf: zero,
                        ..INITIAL_FLAGS
                    },
                    PRESERVED_FLAGS,
                )
                .preserve_flag_record()
                .register(
                    Gpr32::Eax,
                    0x4433_2211,
                    if zero { replacement } else { 0x4433_2211 },
                )
                .initial_register(Gpr32::Ebx, address)
                .map_page(4, 0x8000, ReadOnly)
                .backing(0x8ffb, &[0x5a, 0xa1, 0x88, 0x34, 0x12]),
            );
        }
    }
    for (code, zero, address, eax) in [
        (&[0x66, 0x0f, 0x44, 0x03][..], false, 0x4fff, 0x4433_2211),
        (&[0x0f, 0x44, 0x03][..], true, 0x4ffe, 0x1234_88a1),
    ] {
        cases.push(
            Case::new(
                format!("scattered source at {address:x}, ZF={zero}"),
                code,
                Flags {
                    zf: zero,
                    ..INITIAL_FLAGS
                },
                PRESERVED_FLAGS,
            )
            .preserve_flag_record()
            .register(Gpr32::Eax, 0x4433_2211, eax)
            .initial_register(Gpr32::Ebx, address)
            .map_page(4, 0x8000, ReadOnly)
            .map_page(5, 0xa000, ReadOnly)
            .backing(0x8ffd, &[0x5a, 0xa1, 0x88])
            .backing(0xa000, &[0x34, 0x12, 0x5a]),
        );
    }
    for (zero, eax) in [(false, 0x8000_4020), (true, 0x8000_88a1)] {
        cases.push(
            Case::new(
                format!("word destination is its full address, ZF={zero}"),
                &[0x66, 0x0f, 0x44, 0x00],
                Flags {
                    zf: zero,
                    ..INITIAL_FLAGS
                },
                PRESERVED_FLAGS,
            )
            .preserve_flag_record()
            .register(Gpr32::Eax, 0x8000_4020, eax)
            .map_page(0x80004, 0x8000, ReadOnly)
            .backing(0x801f, &[0x5a, 0xa1, 0x88, 0x5a]),
        );
    }
    for (code, address, mapped, fault) in [
        (&[0x66, 0x0f, 0x45, 0x03][..], 0x4020, false, 0x4020),
        (&[0x66, 0x0f, 0x45, 0x03][..], 0x4fff, true, 0x5000),
        (&[0x0f, 0x45, 0x03][..], 0x4ffe, true, 0x5000),
    ] {
        let mut case = Case::new(
            format!("false condition still reads {address:x}"),
            code,
            INITIAL_FLAGS,
            PRESERVED_FLAGS,
        )
        .preserve_flag_record()
        .initial_register(Gpr32::Ebx, address)
        .backing(0x8ffe, &[0xa1, 0x88])
        .fault(fault, 0);
        if mapped {
            case = case.map_page(4, 0x8000, ReadOnly);
        }
        cases.push(case);
    }
    cases
}

test_cases!(source_accesses_and_faults, source_cases());

test_sequences!(
    local_conditions_before_faults,
    [
        SequenceCase::new("known false CMOVNE still checks its source", INITIAL_FLAGS)
            .initial_register(Gpr32::Ebx, 0x4000)
            .step(
                Checkpoint::new(
                    &[0x31, 0xc0],
                    Flags {
                        cf: Clear,
                        pf: Set,
                        af: Undefined,
                        zf: Set,
                        sf: Clear,
                        of: Clear
                    }
                )
                .register(Gpr32::Eax, 0)
            )
            .step(Checkpoint::preserving_flags(&[0x0f, 0x45, 0x0b]).fault(0x4000, 0)),
        SequenceCase::new(
            "local signed and unsigned conditions before a false source fault",
            INITIAL_FLAGS
        )
        .initial_registers(&[
            (Gpr32::Eax, 0x7fff_fffe),
            (Gpr32::Ebx, 0xffff_fffe),
            (Gpr32::Ecx, 0x8877_6655),
            (Gpr32::Edx, 0xccbb_aa99),
            (Gpr32::Esi, 0x8000_4020)
        ])
        .map_page(0x80004, 0x8000, ReadOnly)
        .backing(0x801f, &[0x5a, 0x78, 0x56, 0x34, 0x12, 0x5a])
        .step(Checkpoint::new(
            &[0x39, 0xd8],
            Flags {
                cf: Set,
                pf: Set,
                af: Clear,
                zf: Clear,
                sf: Set,
                of: Set
            }
        ))
        .step(
            Checkpoint::preserving_flags(&[0x66, 0x0f, 0x4f, 0xd1])
                .register(Gpr32::Edx, 0xccbb_6655)
        )
        .step(Checkpoint::preserving_flags(&[0x0f, 0x4c, 0xc1]))
        .step(Checkpoint::preserving_flags(&[0x0f, 0x42, 0x36]).register(Gpr32::Esi, 0x1234_5678))
        .step(Checkpoint::preserving_flags(&[0x66, 0x0f, 0x4c, 0x13]).fault(0xffff_fffe, 0))
    ]
);
