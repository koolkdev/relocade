use super::*;

fn stack_faults() -> Vec<Case> {
    let mut cases = Vec::new();
    for code in [&[0xc9][..], &[0x66, 0xc9]] {
        for (name, ss, frame_pointer) in [
            ("unusable SS", StoredSegment::unusable(0), 0x4000),
            (
                "last byte exceeds SS limit",
                stack_segment(0x20000, 0x4fff, true),
                0x4fff,
            ),
            (
                "16-bit stack limit precedes paging",
                stack_segment(0x20000, 0x4fff, false),
                0x1234_4fff,
            ),
            (
                "expand-down lower limit",
                StoredSegment {
                    attributes: SegmentAttributes::from_bits(0x1d),
                    ..stack_segment(0, 0x4fff, true)
                },
                0x4fff,
            ),
            (
                "expand-down 16-bit ceiling",
                StoredSegment {
                    attributes: SegmentAttributes::from_bits(0x0d),
                    ..stack_segment(0, 0x7fff, false)
                },
                0x1234_ffff,
            ),
        ] {
            cases.push(
                Case::preserving_flags(
                    format!("LEAVE {code:02x?}: {name} preserves both pointers"),
                    code,
                )
                .segmented_only()
                .segment(Segment::Ss, ss)
                .initial_register(Esp, 0xabcd_9000)
                .initial_register(Ebp, frame_pointer)
                .stack_fault(0),
            );
        }
    }
    cases
}

fn page_faults() -> Vec<Case> {
    let mut cases = Vec::new();
    for (code, big, offset) in [
        (&[0xc9][..], true, 0x1234_4fff),
        (&[0x67, 0x66, 0xc9][..], true, 0x1234_4fff),
        (&[0xc9][..], false, 0x4fff),
        (&[0x67, 0x66, 0xc9][..], false, 0x4fff),
    ] {
        let linear = 0x20000 + offset;
        for first_present in [false, true] {
            let mut case = Case::preserving_flags(
                format!("LEAVE B={big} {code:02x?}, first page present={first_present}"),
                code,
            )
            .segmented_only()
            .segment(Segment::Ss, stack_segment(0x20000, u32::MAX, big))
            .initial_register(Esp, 0xabcd_9000)
            .initial_register(Ebp, 0x1234_4fff)
            .fault(if first_present { linear + 1 } else { linear }, 0);
            if first_present {
                case = case.memory(linear, &[0xef], ReadOnly);
            }
            cases.push(case);
        }
    }
    cases.push(
        Case::preserving_flags(
            "LEAVE retains entry ESP and EBP on a wrapped second-page fault",
            &[0xc9],
        )
        .initial_register(Esp, 0x9000)
        .initial_register(Ebp, 0xffff_fffe)
        .memory(0xffff_fffe, &[0xef, 0xcd], ReadOnly)
        .fault(0, 0),
    );
    cases
}

test_cases!(
    segment_faults_precede_paging_and_pointer_commitment,
    stack_faults()
);
test_cases!(page_faults_preserve_entry_state, page_faults());
