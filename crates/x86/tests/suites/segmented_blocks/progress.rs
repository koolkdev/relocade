use super::code;
use crate::support::{
    cases::{
        FlagExpectation::{Clear, Set},
        Flags,
        Permissions::{ReadOnly, ReadWrite},
    },
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Case},
};
use wasm86_x86::{
    CpuState,
    Gpr32::{Eax, Ebx, Ecx, Edi, Esi, Esp},
    Segment, SegmentAttributes,
    SegmentDefaultSize::{Bits16, Bits32},
    StoredSegment,
};

fn before_add() -> Flags<bool> {
    Flags {
        cf: false,
        pf: false,
        af: false,
        zf: false,
        sf: false,
        of: false,
    }
}

fn add() -> Step {
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
    .register(Eax, 0xaaaa_0080)
}

fn data_fault_progress() -> Vec<Case> {
    [Bits16, Bits32]
        .into_iter()
        .map(|size| {
            let load = if size == Bits16 {
                [0x8b, 0x07]
            } else {
                [0x8b, 0x03]
            };
            Case::new(
                format!("{size:?} data fault preserves completed ADD at the end of valid CS"),
                before_add(),
            )
            .segmented_only()
            .segment(Segment::Cs, code(0x4000, 0x1003, size))
            .segment(
                Segment::Ds,
                StoredSegment {
                    base: 0x6000,
                    ..StoredSegment::flat_data32(0x23)
                },
            )
            .initial_registers(&[(Eax, 0xaaaa_007f), (Ebx, 0x4000)])
            .step(add())
            .step(Step::preserving_flags(&load).fault(0xa000, 0))
        })
        .collect()
}

test_sequences!(
    valid_snapshots_preserve_prior_flags_at_a_data_fault,
    data_fault_progress()
);

test_sequences!(
    size_prefixes_reset_at_each_snapshot_instruction,
    vec![Case::preserving_flags(
        "16-bit code resets operand and address overrides between instructions"
    )
    .segmented_only()
    .segment(Segment::Cs, code(0x6003, 0x100f, Bits16))
    .segment(
        Segment::Ds,
        StoredSegment {
            base: 0x4000,
            ..StoredSegment::flat_data32(0x23)
        }
    )
    .initial_registers(&[(Ebx, 0x0001_0020), (Esi, 0x0002_0004)])
    .memory(0x14024, &[0x11, 0x11], ReadOnly)
    .memory(0x4028, &[0x22, 0x22], ReadOnly)
    .step(Step::preserving_flags(&[0x66, 0xb8, 0x78, 0x56, 0x34, 0x12]).register(Eax, 0x1234_5678))
    .step(Step::preserving_flags(&[0xb8, 0xcd, 0xab]).register(Eax, 0x1234_abcd))
    .step(Step::preserving_flags(&[0x67, 0x8b, 0x43, 4]).register(Eax, 0x1234_1111))
    .step(Step::preserving_flags(&[0x8b, 0x40, 4]).register(Eax, 0x1234_2222)),]
);

fn stack_wrap() -> Vec<Case> {
    let mut cases = Vec::new();
    for size in [Bits16, Bits32] {
        let width = if size == Bits16 { 2 } else { 4 };
        let mut immediate = vec![0xb8, 0x78, 0x56, 0x34, 0x12];
        if size == Bits16 {
            immediate.insert(0, 0x66);
        }
        cases.push(
            Case::preserving_flags(format!(
                "{size:?} block retains stack progress across 16-bit wrapping and SS fault"
            ))
            .segmented_only()
            .segment(Segment::Cs, code(0x6003, 0x1fff, size))
            .segment(
                Segment::Ss,
                StoredSegment {
                    base: 0x8000,
                    limit: 0x7fff,
                    attributes: SegmentAttributes::from_bits(0x0d),
                    ..StoredSegment::flat_data32(0x23)
                },
            )
            .initial_registers(&[(Esp, 0xabcd_0000), (Ebx, 0xbbbb_9999)])
            .memory(0x18000 - width, &vec![0; width as usize], ReadWrite)
            .step(Step::preserving_flags(&immediate).register(Eax, 0x1234_5678))
            .step(
                Step::preserving_flags(&[0x50])
                    .register(Esp, 0xabce_0000 - width)
                    .expect_memory(
                        0x18000 - width,
                        &0x1234_5678u32.to_le_bytes()[..width as usize],
                    ),
            )
            .step(
                Step::preserving_flags(&[0x5b])
                    .register(Esp, 0xabcd_0000)
                    .register(
                        Ebx,
                        if size == Bits16 {
                            0xbbbb_5678
                        } else {
                            0x1234_5678
                        },
                    ),
            )
            .step(Step::preserving_flags(&[0x5a]).stack_fault(0)),
        );
    }
    cases
}

test_sequences!(
    stack_aliases_retain_progress_across_wrap_and_fault,
    stack_wrap()
);

fn repeated_progress() -> Vec<Case> {
    let mut flags = CpuState::filled(0xa5).flags;
    flags.bytes.df = 0;
    vec![Case::preserving_flags(
        "pending full-register writes feed 16-bit REP and survive a later segment fault",
    )
    .segmented_only()
    .stored_flags(flags)
    .segment(Segment::Cs, code(0x6003, 0x1fff, Bits32))
    .segment(
        Segment::Ds,
        StoredSegment {
            base: 0x8000,
            limit: 0x4fff,
            ..StoredSegment::flat_data32(0x23)
        },
    )
    .segment(
        Segment::Es,
        StoredSegment {
            base: 0x18000,
            limit: 0xffff,
            ..StoredSegment::flat_data32(0x23)
        },
    )
    .memory(0xcfff, &[0x66], ReadOnly)
    .memory(0x18000, &[0xaa; 3], ReadWrite)
    .step(Step::preserving_flags(&[0xb9, 3, 0, 0xcd, 0xab]).register(Ecx, 0xabcd_0003))
    .step(Step::preserving_flags(&[0xbe, 0xff, 0x4f, 0xbb, 0xbb]).register(Esi, 0xbbbb_4fff))
    .step(Step::preserving_flags(&[0xbf, 0, 0, 0xcc, 0xcc]).register(Edi, 0xcccc_0000))
    .step(
        Step::preserving_flags(&[0x67, 0xf3, 0xa4])
            .register(Ecx, 0xabcd_0002)
            .register(Esi, 0xbbbb_5000)
            .register(Edi, 0xcccc_0001)
            .expect_memory(0x18000, &[0x66])
            .general_protection(0),
    )]
}

test_sequences!(
    repeated_elements_publish_pending_aliases_at_a_segment_fault,
    repeated_progress()
);
