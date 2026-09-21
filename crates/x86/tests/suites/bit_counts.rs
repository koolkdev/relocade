//! Population counts use the complete logical source and replace all status flags.

use crate::support::{
    cases::{
        test_cases,
        FlagExpectation::{self, Clear, Set},
        Flags, InstructionCase as Case,
        Permissions::ReadOnly,
    },
    encoding::check_length,
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Sequence},
};
use wasm86_x86::{Gpr32::*, Segment, SegmentAttributes, StoredSegment};

const NONZERO: Flags<FlagExpectation> = Flags {
    cf: Clear,
    pf: Clear,
    af: Clear,
    zf: Clear,
    sf: Clear,
    of: Clear,
};
const ZERO: Flags<FlagExpectation> = Flags { zf: Set, ..NONZERO };

fn register_counts() -> Vec<Case> {
    let mut cases = Vec::new();
    for (source, count, flags) in [
        (0, 0, ZERO),
        (u32::MAX, 32, NONZERO),
        (0x8000_0000, 1, NONZERO),
        (0xffff_0000, 16, NONZERO),
        (0xa5a5_1234, 13, NONZERO),
        (0x0101_0101, 4, NONZERO),
    ] {
        cases.push(
            Case::replacing_flags(
                format!("POPCNT EAX,EDX: {source:08x}"),
                &[0xf3, 0x0f, 0xb8, 0xc2],
                flags,
            )
            .register(Eax, 0x4433_2211, count)
            .initial_register(Edx, source),
        );
    }
    for (source, count, flags) in [
        (0xffff_0000, 0, ZERO),
        (0xffff_0001, 1, NONZERO),
        (0xabcd_ffff, 16, NONZERO),
        (0x1234_8000, 1, NONZERO),
        (0xff00_1234, 5, NONZERO),
    ] {
        cases.push(
            Case::replacing_flags(
                format!("POPCNT AX,DX: {source:08x}"),
                &[0x66, 0xf3, 0x0f, 0xb8, 0xc2],
                flags,
            )
            .register(Eax, 0x4433_2211, 0x4433_0000 | count)
            .initial_register(Edx, source),
        );
    }
    cases.extend([
        Case::replacing_flags(
            "POPCNT CX,CX reads before replacing its low word",
            &[0xf3, 0x66, 0x0f, 0xb8, 0xc9],
            NONZERO,
        )
        .register(Ecx, 0xabcd_ffff, 0xabcd_0010),
        Case::replacing_flags(
            "POPCNT EAX,EAX counts its old full value",
            &[0xf3, 0x0f, 0xb8, 0xc0],
            NONZERO,
        )
        .register(Eax, u32::MAX, 32),
        Case::replacing_flags(
            "16-bit code counts only the source word",
            &[0xf3, 0x0f, 0xb8, 0xc2],
            ZERO,
        )
        .register(Eax, 0x1234_5678, 0x1234_0000)
        .initial_register(Edx, 0xffff_0000)
        .segmented_only()
        .segment(
            Segment::Cs,
            StoredSegment {
                attributes: SegmentAttributes::from_bits(0x07),
                ..StoredSegment::flat_code32(0x1b)
            },
        ),
    ]);
    cases
}

fn memory_sources() -> Vec<Case> {
    vec![
        Case::replacing_flags(
            "POPCNT reads every byte of a scattered dword",
            &[0xf3, 0x0f, 0xb8, 0x03],
            NONZERO,
        )
        .register(Eax, 0x4433_2211, 13)
        .initial_register(Ebx, 0x4ffe)
        .map_page(4, 0x8000, ReadOnly)
        .map_page(5, 0xa000, ReadOnly)
        .memory(0x4ffe, &[0xff, 0x0f, 0, 0x80], ReadOnly),
        Case::replacing_flags(
            "POPCNT AX,[EAX] reads the old address and only two bytes",
            &[0x66, 0xf3, 0x0f, 0xb8, 0x00],
            NONZERO,
        )
        .register(Eax, 0x8000_4ffe, 0x8000_0003)
        .memory(0x8000_4ffe, &[3, 0x80], ReadOnly),
        Case::replacing_flags(
            "F3 memory decoding carries FS and a wrapping 16-bit address",
            &[0x64, 0x67, 0xf3, 0x0f, 0xb8, 0x00],
            NONZERO,
        )
        .register(Eax, 0x4433_2211, 4)
        .initial_registers(&[(Ebx, 0xdead_fff0), (Esi, 0xbeef_0014)])
        .segment(
            Segment::Fs,
            StoredSegment {
                base: 0x4000,
                limit: 0xfff,
                ..StoredSegment::flat_data32(0x33)
            },
        )
        .memory(0x4004, &[0x81, 0x80, 0x08, 0], ReadOnly),
        Case::preserving_flags(
            "POPCNT must read the missing high half before changing state",
            &[0xf3, 0x0f, 0xb8, 0x03],
        )
        .initial_register(Ebx, 0x4ffe)
        .memory(0x4ffe, &[0xff, 0xff], ReadOnly)
        .fault(0x5000, 0),
    ]
}

#[test]
fn counts_consume_a_complete_source_encoding_without_an_immediate() {
    for code in [
        &[0xf3, 0x0f, 0xb8, 0xc2][..],
        &[0x66, 0xf3, 0x0f, 0xb8, 0xc9],
        &[0xf3, 0x0f, 0xb8, 0x03],
        &[0x64, 0xf3, 0x0f, 0xb8, 0x44, 0x8b, 0xfc],
        &[0x67, 0xf3, 0x0f, 0xb8, 0x06, 0, 0x40],
    ] {
        check_length(code);
    }
}

test_cases!(logical_source_counts_and_status_flags, register_counts());
test_cases!(memory_sources_complete_before_publication, memory_sources());

test_sequences!(
    pending_flags_and_source_dependencies,
    [Sequence::from_opaque_flags(
        "POPCNT replaces pending flags and publishes before a later source fault"
    )
    .initial_registers(&[
        (Eax, 0xaabb_1234),
        (Ebx, u32::MAX),
        (Ecx, 0x4444_44ff),
        (Edx, 0xffff_8001),
        (Esi, 0x5001)
    ])
    .step(
        Step::new(
            &[0x83, 0xc3, 1],
            Flags {
                cf: Set,
                pf: Set,
                af: Set,
                zf: Set,
                sf: Clear,
                of: Clear,
            }
        )
        .register(Ebx, 0)
    )
    .step(Step::new(&[0x66, 0xf3, 0x0f, 0xb8, 0xc2], NONZERO).register(Eax, 0xaabb_0002))
    .step(Step::preserving_flags(&[0x0f, 0x9a, 0xc1]).register(Ecx, 0x4444_4400))
    .step(Step::new(&[0xf3, 0x0f, 0xb8, 0xd2], NONZERO).register(Edx, 18))
    .step(Step::preserving_flags(&[0xf3, 0x0f, 0xb8, 0x06]).fault(0x5001, 0))]
);
