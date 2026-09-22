//! Fixed-width register pairs, ZF-only updates and complete memory write checks.

use crate::support::{
    cases::{
        test_cases,
        FlagExpectation::{Clear, Preserved, Set},
        Flags, InstructionCase as Case,
        Permissions::{ReadOnly, ReadWrite},
    },
    encoding::check_length,
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Sequence},
};
use wasm86_x86::{Gpr32::*, Segment, SegmentAttributes, StoredSegment};

const ORIGINAL: [u8; 8] = [0x88, 0x77, 0x66, 0x55, 0x44, 0x33, 0x22, 0x11];
const REPLACEMENT: [u8; 8] = [0x10, 0x32, 0x54, 0x76, 0x98, 0xba, 0xdc, 0xfe];

fn successful_exchange(name: &str, code: &[u8], flags: bool) -> Case {
    Case::new(
        name,
        code,
        Flags::all(flags),
        Flags {
            zf: Set,
            ..Flags::all(Preserved)
        },
    )
    .initial_registers(&[
        (Eax, 0x5566_7788),
        (Edx, 0x1122_3344),
        (Ebx, 0x7654_3210),
        (Ecx, 0xfedc_ba98),
        (Edi, 0x4000),
    ])
    .memory(0x4000, &ORIGINAL, ReadWrite)
    .expect_memory(0x4000, &REPLACEMENT)
}

fn comparisons() -> Vec<Case> {
    let mut cases = Vec::new();
    for flags in [false, true] {
        cases.push(successful_exchange(
            "equal complete register pair",
            &[0x0f, 0xc7, 0x0f],
            flags,
        ));
        for (name, high, low) in [
            ("high half differs", 0x9122_3344, 0x5566_7788),
            ("low half differs", 0x1122_3344, 0x5566_7789),
        ] {
            cases.push(
                Case::new(
                    name,
                    &[0x0f, 0xc7, 0x0f],
                    Flags::all(flags),
                    Flags {
                        zf: Clear,
                        ..Flags::all(Preserved)
                    },
                )
                .register(Eax, low, 0x5566_7788)
                .register(Edx, high, 0x1122_3344)
                .initial_registers(&[(Ebx, 0x7654_3210), (Ecx, 0xfedc_ba98), (Edi, 0x4000)])
                .memory(0x4000, &ORIGINAL, ReadWrite),
            );
        }
    }
    cases.push(successful_exchange(
        "66 keeps the full eight-byte comparison and store",
        &[0x66, 0x0f, 0xc7, 0x0f],
        false,
    ));
    for code in [
        &[0x0f, 0xc7, 0x0e, 0, 0x40][..],
        &[0x66, 0x0f, 0xc7, 0x0e, 0, 0x40],
    ] {
        cases.push(
            successful_exchange("16-bit code still uses full register pairs", code, false)
                .segmented_only()
                .segment(
                    Segment::Cs,
                    StoredSegment {
                        attributes: SegmentAttributes::from_bits(0x07),
                        ..StoredSegment::flat_code32(0x1b)
                    },
                ),
        );
    }
    cases
}

fn memory_updates() -> Vec<Case> {
    vec![
        Case::new(
            "mismatch keeps the address formed by both accumulator registers",
            &[0x0f, 0xc7, 0x0c, 0x90],
            Flags::all(true),
            Flags {
                zf: Clear,
                ..Flags::all(Preserved)
            },
        )
        .register(Eax, 0x4000, 0x5566_7788)
        .register(Edx, 1, 0x1122_3344)
        .initial_registers(&[(Ebx, 0x7654_3210), (Ecx, 0xfedc_ba98)])
        .memory(0x4004, &ORIGINAL, ReadWrite),
        Case::new(
            "LOCK exchanges an unaligned qword across scattered frames",
            &[0xf0, 0x0f, 0xc7, 0x0f],
            Flags::all(false),
            Flags {
                zf: Set,
                ..Flags::all(Preserved)
            },
        )
        .initial_registers(&[
            (Eax, 0x5566_7788),
            (Edx, 0x1122_3344),
            (Ebx, 0x7654_3210),
            (Ecx, 0xfedc_ba98),
            (Edi, 0x4ffd),
        ])
        .map_page(4, 0x8000, ReadWrite)
        .map_page(5, 0xa000, ReadWrite)
        .memory(0x4ffd, &ORIGINAL, ReadWrite)
        .expect_memory(0x4ffd, &REPLACEMENT),
        Case::new(
            "LOCK carries FS and wraps the 16-bit starting address",
            &[0x64, 0xf0, 0x67, 0x0f, 0xc7, 0x08],
            Flags::all(false),
            Flags {
                zf: Set,
                ..Flags::all(Preserved)
            },
        )
        .initial_registers(&[
            (Eax, 0x5566_7788),
            (Edx, 0x1122_3344),
            (Ebx, 0x7654_fff0),
            (Ecx, 0xfedc_ba98),
            (Esi, 0xabcd_0014),
        ])
        .segment(
            Segment::Fs,
            StoredSegment {
                base: 0x4000,
                limit: 0xfff,
                ..StoredSegment::flat_data32(0x33)
            },
        )
        .memory(0x4004, &ORIGINAL, ReadWrite)
        .expect_memory(0x4004, &[0xf0, 0xff, 0x54, 0x76, 0x98, 0xba, 0xdc, 0xfe]),
        Case::preserving_flags(
            "all eight bytes must fit the segment before publication",
            &[0x64, 0x0f, 0xc7, 0x0f],
        )
        .initial_register(Edi, 0x100)
        .segment(
            Segment::Fs,
            StoredSegment {
                base: 0x4000,
                limit: 0x106,
                ..StoredSegment::flat_data32(0x33)
            },
        )
        .memory(0x4100, &ORIGINAL, ReadWrite)
        .general_protection(0),
    ]
}

fn write_faults() -> Vec<Case> {
    let mut cases = Vec::new();
    for accumulator_low in [0x5566_7788, 0x5566_7789] {
        cases.push(
            Case::preserving_flags(
                "both comparison outcomes require writable memory",
                &[0x0f, 0xc7, 0x0f],
            )
            .initial_registers(&[(Eax, accumulator_low), (Edx, 0x1122_3344), (Edi, 0x4001)])
            .memory(0x4001, &ORIGINAL, ReadOnly)
            .fault(0x4001, 3),
        );
        cases.push(
            Case::preserving_flags(
                "a denied high half preserves both registers and flags",
                &[0xf0, 0x0f, 0xc7, 0x0f],
            )
            .initial_registers(&[(Eax, accumulator_low), (Edx, 0x1122_3344), (Edi, 0x4ffc)])
            .memory(0x4ffc, &ORIGINAL[..4], ReadWrite)
            .memory(0x5000, &ORIGINAL[4..], ReadOnly)
            .fault(0x5000, 3),
        );
    }
    cases.push(
        Case::preserving_flags(
            "missing high half reports a write fault before comparison",
            &[0x0f, 0xc7, 0x0f],
        )
        .initial_registers(&[(Eax, 0x5566_7788), (Edx, 0x1122_3344), (Edi, 0x4ffc)])
        .memory(0x4ffc, &ORIGINAL[..4], ReadWrite)
        .fault(0x5000, 2),
    );
    cases
}

test_cases!(complete_comparison_and_fixed_width, comparisons());
test_cases!(memory_addresses_and_complete_updates, memory_updates());
test_cases!(write_checks_precede_comparison_effects, write_faults());

#[test]
fn encoding_requires_one_memory_operand_and_no_immediate() {
    for code in [
        &[0x0f, 0xc7, 0x0f][..],
        &[0x66, 0x0f, 0xc7, 0x0f],
        &[0xf0, 0x0f, 0xc7, 0x8c, 0x90, 0x78, 0x56, 0x34, 0x12],
        &[0x67, 0x0f, 0xc7, 0x0e, 0, 0x40],
    ] {
        check_length(code);
    }
}

test_sequences!(
    pending_flags_and_pair_publication,
    [
        Sequence::from_opaque_flags(
            "CMPXCHG8B updates ZF while retaining pending arithmetic flags"
        )
        .initial_registers(&[
            (Eax, 0),
            (Edx, 0),
            (Ebp, u32::MAX),
            (Edi, 0x4000),
            (Ebx, 0x7654_3210),
            (Ecx, 0xfedc_ba98),
            (Esi, 0x5000)
        ])
        .memory(0x4000, &ORIGINAL, ReadWrite)
        .step(
            Step::new(
                &[0x83, 0xc5, 1],
                Flags {
                    cf: Set,
                    pf: Set,
                    af: Set,
                    zf: Set,
                    sf: Clear,
                    of: Clear
                }
            )
            .register(Ebp, 0)
        )
        .step(
            Step::new(
                &[0x0f, 0xc7, 0x0f],
                Flags {
                    zf: Clear,
                    ..Flags::all(Preserved)
                }
            )
            .register(Eax, 0x5566_7788)
            .register(Edx, 0x1122_3344)
        )
        .step(
            Step::new(
                &[0xf0, 0x0f, 0xc7, 0x0f],
                Flags {
                    zf: Set,
                    ..Flags::all(Preserved)
                }
            )
            .expect_memory(0x4000, &REPLACEMENT)
        )
        .step(Step::preserving_flags(&[0x0f, 0x92, 0xc1]).register(Ecx, 0xfedc_ba01))
        .step(Step::preserving_flags(&[0x0f, 0xc7, 0x0e]).fault(0x5000, 2))
    ]
);
