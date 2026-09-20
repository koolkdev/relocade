use super::*;
use wasm86_x86::SegmentAttributes;

fn page_faults() -> Vec<Case> {
    let mut cases = Vec::new();
    for (width, prefix) in [(2, &[0x66][..]), (4, &[][..])] {
        for present in [false, true] {
            let mut push = Case::preserving_flags(
                format!(
                    "PUSHA first split slot is complete or absent, width={width} present={present}"
                ),
                &[prefix, &[0x60]].concat(),
            )
            .initial_registers(&INPUT)
            .initial_register(Esp, 0x4fff + width as u32)
            .fault(if present { 0x5000 } else { 0x4fff }, 2);
            let mut pop = Case::preserving_flags(
                format!(
                    "POPA first split slot preserves entry state, width={width} present={present}"
                ),
                &[prefix, &[0x61]].concat(),
            )
            .initial_registers(&INPUT)
            .initial_register(Esp, 0x4fff)
            .fault(if present { 0x5000 } else { 0x4fff }, 0);
            if present {
                push = push.memory(0x4fff, &[0xa5], ReadWrite);
                pop = pop.memory(0x4fff, &[0xa5], ReadOnly);
            }
            cases.extend([push, pop]);
        }
        let frame = pushed(0x5000 + 4 * width as u32, width);
        cases.push(
            Case::preserving_flags(
                format!("PUSHA retains four completed stores on a later page fault, width={width}"),
                &[prefix, &[0x60]].concat(),
            )
            .initial_registers(&INPUT)
            .initial_register(Esp, 0x5000 + 4 * width as u32)
            .memory(0x5000, &vec![0xa5; 4 * width], ReadWrite)
            .expect_memory(0x5000, &frame[4 * width..])
            .fault(0x5000 - width as u32, 2),
        );
        cases.push(
            Case::preserving_flags(
                format!("PUSHA rejects read-only first slot, width={width}"),
                &[prefix, &[0x60]].concat(),
            )
            .initial_registers(&INPUT)
            .initial_register(Esp, 0x9000 + width as u32)
            .memory(0x9000, &vec![0xa5; width], ReadOnly)
            .fault(0x9000, 3),
        );
    }
    cases
}

fn pop_page_progress() -> Vec<Case> {
    let mut cases = Vec::new();
    for (width, prefix) in [(2, &[0x66][..]), (4, &[][..])] {
        let source = pop_source(width);
        for big in [false, true] {
            let high = if big { 0 } else { 0xabcd_0000 };
            let base = if big { 0 } else { 0x20000 };
            for slot in 0..8 {
                let start = 0x5000 - (slot * width) as u32;
                let mut case = Case::preserving_flags(
                    format!("POPA retains earlier restores on slot {slot} page fault, width={width} B={big}"),
                    &[prefix, &[0x61]].concat(),
                )
                .initial_registers(&INPUT).initial_register(Esp, high | start)
                .segment(Segment::Ss, stack_segment(base, u32::MAX, big))
                .fault(base + 0x5000, 0);
                if slot != 0 {
                    case = case.memory(base + start, &source[..slot * width], ReadOnly);
                }
                if !big {
                    case = case.segmented_only();
                }
                cases.push(expect_restored(case, width == 2, slot));
            }
            let start = 0x5000 - width as u32 - 1;
            let mut case = Case::preserving_flags(
                format!("POPA split second slot leaves only the first register restored, width={width} B={big}"),
                &[prefix, &[0x61]].concat(),
            )
            .initial_registers(&INPUT).initial_register(Esp, high | start)
            .segment(Segment::Ss, stack_segment(base, u32::MAX, big))
            .memory(base + start, &source[..width + 1], ReadOnly)
            .fault(base + 0x5000, 0);
            if !big {
                case = case.segmented_only();
            }
            cases.push(expect_restored(case, width == 2, 1));
        }
        let start = 0x10000 - (3 * width) as u32 - 1;
        cases.push(expect_restored(
            Case::preserving_flags(
                format!("POPA pages the entire discarded slot across 64K, width={width}"),
                &[prefix, &[0x61]].concat(),
            )
            .segmented_only()
            .initial_registers(&INPUT)
            .segment(Segment::Ss, stack_segment(0, 0x2ffff, false))
            .initial_register(Esp, 0xabcd_0000 | start)
            .memory(start, &source[..3 * width + 1], ReadOnly)
            .memory(width as u32 - 1, &source[4 * width..], ReadOnly)
            .fault(0x10000, 0),
            width == 2,
            3,
        ));
        let start = if width == 2 { 0xfffa } else { 0xfff4 };
        let code = if width == 2 {
            vec![0x61]
        } else {
            vec![0x66, 0x61]
        };
        cases.push(expect_restored(
            Case::preserving_flags(
                format!(
                    "16-bit code POPA faults on the discarded slot after stack wrap, width={width}"
                ),
                &code,
            )
            .segmented_only()
            .initial_registers(&INPUT)
            .segment(Segment::Cs, code16())
            .segment(Segment::Ss, stack_segment(0, 0xffff, false))
            .initial_register(Esp, 0xabcd_0000 | start)
            .instruction_count(u32::MAX)
            .memory(start, &source[..3 * width], ReadOnly)
            .fault(0, 0),
            width == 2,
            3,
        ));
    }
    cases
}

fn segment_faults() -> Vec<Case> {
    let mut cases = Vec::new();
    for big in [false, true] {
        let high = if big { 0 } else { 0xabcd_0000 };
        for opcode in [0x60, 0x61] {
            cases.push(
                Case::preserving_flags(
                    format!("{opcode:02x} first slot SS failure precedes paging, B={big}"),
                    &[opcode],
                )
                .segmented_only()
                .initial_registers(&INPUT)
                .segment(Segment::Ss, stack_segment(0x20000, 0x4fff, big))
                .initial_register(Esp, high | if opcode == 0x60 { 0x5002 } else { 0x4ffe })
                .stack_fault(0),
            );
        }
        for (width, prefix, start) in [(2, &[0x66][..], 0x4ffc), (4, &[][..], 0x4ff8)] {
            cases.push(expect_restored(
                Case::preserving_flags(
                    format!("POPA keeps two restored registers on a later SS fault, width={width} B={big}"),
                    &[prefix, &[0x61]].concat(),
                )
                .segmented_only()
                .initial_registers(&INPUT)
                .segment(Segment::Ss, stack_segment(0x20000, 0x4fff, big))
                .initial_register(Esp, high | start)
                .memory(0x20000 + start, &pop_source(width)[..2 * width], ReadOnly)
                .stack_fault(0),
                width == 2,
                2,
            ));
        }
        for present in [false, true] {
            let mut case = Case::preserving_flags(
                format!("PUSHA checks each slot in order before a later expand-down failure, B={big} present={present}"), &[0x60],
            ).segmented_only().initial_registers(&INPUT)
                .segment(Segment::Ss, StoredSegment {
                    attributes: SegmentAttributes::from_bits(if big { 0x1d } else { 0x0d }),
                    ..stack_segment(0x20000, 0x8fff, big)
                }).initial_register(Esp, high | 0x9004);
            case = if present {
                case.memory(0x29000, &[0xa5; 4], ReadWrite)
                    .expect_memory(0x29000, &[0x22, 0x22, 0x11, 0x11])
                    .stack_fault(0)
            } else {
                case.fault(0x29000, 2)
            };
            cases.push(case);
        }
    }
    cases.push(expect_restored(
        Case::preserving_flags(
            "POPA checks the skipped ESP slot's complete SS span",
            &[0x61],
        )
        .segmented_only()
        .initial_registers(&INPUT)
        .segment(Segment::Ss, stack_segment(0, 0xffff, false))
        .initial_register(Esp, 0xabcd_fff2)
        .memory(0xfff2, &pop_source(4)[..12], ReadOnly)
        .stack_fault(0),
        false,
        3,
    ));
    for opcode in [0x60, 0x61] {
        cases.push(
            Case::preserving_flags(format!("{opcode:02x} requires usable SS"), &[opcode])
                .segmented_only()
                .initial_registers(&INPUT)
                .initial_register(Esp, 0x9000)
                .segment(Segment::Ss, StoredSegment::unusable(0))
                .stack_fault(0),
        );
    }
    cases
}

test_cases!(ordered_page_access_and_fault_publication, page_faults());
test_cases!(
    pop_faults_retain_completed_register_restores,
    pop_page_progress()
);
test_cases!(
    per_slot_segment_checks_and_fault_publication,
    segment_faults()
);
