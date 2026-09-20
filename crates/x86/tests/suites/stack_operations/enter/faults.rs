use super::*;

fn push_faults() -> Vec<Case> {
    let mut cases = Vec::new();
    for (code, esp) in [
        (&[0xc8, 0, 0, 0][..], 0x5003),
        (&[0x66, 0xc8, 0, 0, 0][..], 0x5001),
    ] {
        for present in [false, true] {
            let mut case = Case::preserving_flags(
                format!("ENTER first push fault {code:02x?}, first page={present}"),
                code,
            )
            .initial_register(Esp, esp)
            .initial_register(Ebp, 0x1234_5678)
            .fault(if present { 0x5000 } else { 0x4fff }, 2);
            if present {
                case = case.memory(0x4fff, &[0xa5], ReadWrite);
            }
            cases.push(case);
        }
    }
    cases.push(
        Case::preserving_flags(
            "ENTER rejects a read-only first push before transferring bytes",
            &[0xc8, 0, 0, 0],
        )
        .initial_register(Esp, 0x9004)
        .initial_register(Ebp, 0x1234_5678)
        .memory(0x9000, &[0xa5; 4], ReadOnly)
        .fault(0x9000, 3),
    );
    cases
}

fn partial_frames() -> Vec<Case> {
    vec![
        Case::preserving_flags(
            "ENTER retains its first push on a display read fault",
            &[0xc8, 0, 0, 2],
        )
        .initial_register(Esp, 0x9000)
        .initial_register(Ebp, 0x7000)
        .memory(0x8ff4, &[0xa5; 12], ReadWrite)
        .expect_memory(0x8ffc, &[0, 0x70, 0, 0])
        .fault(0x6ffc, 0),
        Case::preserving_flags(
            "ENTER retains copied frames on a later display read fault",
            &[0xc8, 0, 0, 3],
        )
        .initial_register(Esp, 0x9000)
        .initial_register(Ebp, 0x5004)
        .memory(0x5000, &[0x78, 0x56, 0x34, 0x12], ReadOnly)
        .memory(0x8ff0, &[0xa5; 16], ReadWrite)
        .expect_memory(0x8ff8, &[0x78, 0x56, 0x34, 0x12, 4, 0x50, 0, 0])
        .fault(0x4ffc, 0),
        Case::preserving_flags(
            "ENTER retains the saved pointer when a copied-frame push faults",
            &[0xc8, 0, 0, 2],
        )
        .initial_register(Esp, 0x5004)
        .initial_register(Ebp, 0x9004)
        .memory(0x5000, &[0xa5; 4], ReadWrite)
        .memory(0x9000, &[0x78, 0x56, 0x34, 0x12], ReadOnly)
        .expect_memory(0x5000, &[4, 0x90, 0, 0])
        .fault(0x4ffc, 2),
        Case::preserving_flags(
            "ENTER self-link push may fault after the saved pointer",
            &[0xc8, 0, 0, 1],
        )
        .initial_register(Esp, 0x5004)
        .initial_register(Ebp, 0x9004)
        .memory(0x5000, &[0xa5; 4], ReadWrite)
        .expect_memory(0x5000, &[4, 0x90, 0, 0])
        .fault(0x4ffc, 2),
        Case::preserving_flags(
            "word ENTER retains only completed pushes on a split display read fault",
            &[0x66, 0xc8, 0, 0, 2],
        )
        .initial_register(Esp, 0x9000)
        .initial_register(Ebp, 0x5001)
        .memory(0x8ffa, &[0xa5; 6], ReadWrite)
        .memory(0x4fff, &[0x34], ReadOnly)
        .expect_memory(0x8ffe, &[1, 0x50])
        .fault(0x5000, 0),
    ]
}

fn allocation_faults() -> Vec<Case> {
    let mut cases = Vec::new();
    for (code, esp, saved) in [
        (
            &[0xc8, 4, 0x10, 0][..],
            0x9004,
            &[0x78, 0x56, 0x34, 0x12][..],
        ),
        (&[0x66, 0xc8, 4, 0x10, 0][..], 0x9002, &[0x78, 0x56][..]),
    ] {
        for read_only in [false, true] {
            let mut case = Case::preserving_flags(
                format!("ENTER allocation probe {code:02x?}, read-only={read_only}"),
                code,
            )
            .initial_register(Esp, esp)
            .initial_register(Ebp, 0x1234_5678)
            .memory(0x9000, &vec![0xa5; saved.len()], ReadWrite)
            .expect_memory(0x9000, saved)
            .fault(0x7ffc, if read_only { 3 } else { 2 });
            if read_only {
                case = case.memory(0x7ffc, &[0x5a; 4], ReadOnly);
            }
            cases.push(case);
        }
    }
    cases.push(
        Case::preserving_flags(
            "ENTER final write probe checks the complete dword without storing it",
            &[0xc8, 1, 0x10, 0],
        )
        .initial_register(Esp, 0x9004)
        .initial_register(Ebp, 0x1234_5678)
        .memory(0x9000, &[0xa5; 4], ReadWrite)
        .memory(0x7fff, &[0x5a], ReadWrite)
        .expect_memory(0x9000, &[0x78, 0x56, 0x34, 0x12])
        .fault(0x8000, 2),
    );
    cases.push(
        Case::preserving_flags(
            "ENTER final write probe checks the complete word without storing it",
            &[0x66, 0xc8, 1, 0x10, 0],
        )
        .initial_register(Esp, 0x9002)
        .initial_register(Ebp, 0x1234_5678)
        .memory(0x9000, &[0xa5; 2], ReadWrite)
        .memory(0x7fff, &[0x5a], ReadWrite)
        .expect_memory(0x9000, &[0x78, 0x56])
        .fault(0x8000, 2),
    );
    cases
}

fn segment_faults() -> Vec<Case> {
    let mut cases = Vec::new();
    for big in [false, true] {
        cases.push(
            Case::preserving_flags(
                format!("ENTER first push SS span precedes paging, B={big}"),
                &[0xc8, 0, 0, 0],
            )
            .segmented_only()
            .segment(Segment::Ss, stack_segment(0x20000, 0x4fff, big))
            .initial_register(Esp, 0x5002)
            .initial_register(Ebp, 0x7000)
            .stack_fault(0),
        );
        cases.push(
            Case::preserving_flags(
                format!("ENTER display SS failure preserves entry registers, B={big}"),
                &[0xc8, 0, 0, 2],
            )
            .segmented_only()
            .segment(Segment::Ss, stack_segment(0x20000, 0x4fff, big))
            .initial_register(Esp, 0x4004)
            .initial_register(Ebp, 0x6000)
            .memory(0x24000, &[0xa5; 4], ReadWrite)
            .expect_memory(0x24000, &[0, 0x60, 0, 0])
            .stack_fault(0),
        );
        cases.push(
            Case::preserving_flags(
                format!(
                    "ENTER allocation below an expand-down limit faults before paging, B={big}"
                ),
                &[0xc8, 1, 0x20, 0],
            )
            .segmented_only()
            .segment(
                Segment::Ss,
                StoredSegment {
                    attributes: SegmentAttributes::from_bits(if big { 0x1d } else { 0x0d }),
                    ..stack_segment(0x20000, 0x7fff, big)
                },
            )
            .initial_register(Esp, 0x9004)
            .initial_register(Ebp, 0x1234_5678)
            .memory(0x29000, &[0xa5; 4], ReadWrite)
            .expect_memory(0x29000, &[0x78, 0x56, 0x34, 0x12])
            .stack_fault(0),
        );
    }
    cases.push(
        Case::preserving_flags(
            "ENTER with unusable SS fails before any memory access",
            &[0xc8, 0, 0, 0],
        )
        .segmented_only()
        .segment(Segment::Ss, StoredSegment::unusable(0))
        .initial_register(Esp, 0x9000)
        .initial_register(Ebp, 0x7000)
        .stack_fault(0),
    );
    cases
}

test_cases!(first_push_faults_preserve_entry_state, push_faults());
test_cases!(
    later_faults_retain_completed_memory_writes,
    partial_frames()
);
test_cases!(allocated_stack_requires_write_access, allocation_faults());
test_cases!(segment_checks_precede_each_page_access, segment_faults());
