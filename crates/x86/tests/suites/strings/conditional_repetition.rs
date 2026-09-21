//! Conditional repeats publish the last comparison on success and entry flags on faults.

use super::{flags, record};
use crate::support::{
    cases::{test_cases, InstructionCase as Case, Permissions::ReadOnly},
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Sequence},
};
use wasm86_x86::{Gpr32::*, Segment, StoredSegment};

#[rustfmt::skip]
fn stopping_conditions() -> Vec<Case> {
    let mut cases = Vec::new();
    for (name, opcode) in [("CMPS", 0xa6), ("SCAS", 0xae)] {
        for (prefix, width, mut right, consumed, status) in [
            (0xf3, 1, [3u32, 5, 5], 1, 0),
            (0xf3, 2, [5, 5, 3], 3, 0),
            (0xf3, 4, [5, 5, 5], 3, 10),
            (0xf2, 1, [5, 3, 3], 1, 10),
            (0xf2, 2, [3, 3, 5], 3, 10),
            (0xf2, 4, [3, 3, 3], 3, 0),
        ] {
            let backward = width == 2;
            let start_offset = if backward { 4 } else { 0 };
            let next_offset = if backward { 0u32.wrapping_sub(2) } else { width * consumed };
            let code = match width {
                1 => vec![prefix, opcode],
                2 => vec![prefix, 0x66, opcode + 1],
                _ => vec![prefix, opcode + 1],
            };
            let mut stored = record(u8::from(backward));
            stored.bytes.zf = u8::from(prefix == 0xf2); // Opposite the condition required to continue.
            if backward { right.reverse(); }
            let right_bytes: Vec<_> = right.iter().flat_map(|value| value.to_le_bytes()[..width as usize].to_vec()).collect();
            let mut case = Case::replacing_flags(format!("{name} {prefix:02x}, width={width}, consumes={consumed}"), &code, flags(status))
                .stored_flags(stored).initial_register(Eax, if width == 4 { 5 } else { 0xaabb_0005 })
                .register(Ecx, 3, 3 - consumed).register(Edi, 0x6000 + start_offset, 0x6000u32.wrapping_add(next_offset))
                .memory(0x6000, &right_bytes, ReadOnly);
            if opcode == 0xa6 {
                case = case.register(Esi, 0x4000 + start_offset, 0x4000u32.wrapping_add(next_offset))
                    .memory(0x4000, &5u32.to_le_bytes()[..width as usize].repeat(3), ReadOnly);
            }
            cases.push(case);
        }
    }
    cases.extend([
        Case::replacing_flags("REPNE SCASD publishes all flags from its final unequal comparison", &[0xf2, 0xaf], flags(38))
            .stored_flags(record(0)).initial_register(Eax, 0x8000_0000).register(Ecx, 2, 0).register(Edi, 0x6000, 0x6008)
            .memory(0x6000, &[0xff, 0xff, 0xff, 0xff, 1, 0, 0, 0], ReadOnly),
        Case::replacing_flags("REPE CMPSW stops at inequality without touching the absent next page", &[0xf3, 0x66, 0xa7], flags(23))
            .stored_flags(record(0)).register(Ecx, 3, 2).register(Esi, 0x4ffe, 0x5000).register(Edi, 0x7ffe, 0x8000)
            .memory(0x4ffe, &[0, 0], ReadOnly).memory(0x7ffe, &[1, 0], ReadOnly),
        Case::replacing_flags("REPNE SCASB stops at equality without touching the absent next page", &[0xf2, 0xae], flags(10))
            .stored_flags(record(0)).initial_register(Eax, 5).register(Ecx, 3, 2).register(Edi, 0x7fff, 0x8000)
            .memory(0x7fff, &[5], ReadOnly),
    ]);
    cases
}

#[rustfmt::skip]
fn zero_count() -> Vec<Case> {
    [
        (&[0xf2, 0xa6][..], 0), (&[0xf3, 0x66, 0xa7][..], 0),
        (&[0x67, 0xf2, 0xaf][..], 0xaaaa_0000), (&[0x67, 0xf3, 0xae][..], 0xaaaa_0000),
    ].into_iter().map(|(code, count)| {
        let mut stored = record(1);
        stored.status_source.kind = 9;
        Case::preserving_flags(format!("zero comparison count skips unusable segments: {code:02x?}"), code)
            .stored_flags(stored).segmented_only().initial_registers(&[(Ecx, count), (Esi, u32::MAX), (Edi, u32::MAX)])
            .segment(Segment::Ds, StoredSegment::unusable(0)).segment(Segment::Es, StoredSegment::unusable(0))
    }).collect()
}

#[rustfmt::skip]
fn fault_rollback() -> Vec<Case> {
    vec![
        Case::preserving_flags("repeated CMPS source fault precedes its missing ES operand", &[0xf3, 0xa6])
            .stored_flags(record(0)).initial_registers(&[(Ecx, 2), (Esi, 0x5000), (Edi, 0x8000)]).fault(0x5000, 0),
        Case::preserving_flags("REPE CMPSD restores entry flags after two comparisons and a source fault", &[0xf3, 0xa7])
            .stored_flags(record(0)).register(Ecx, 3, 1).register(Esi, 0x4ff8, 0x5000).register(Edi, 0x7000, 0x7008)
            .memory(0x4ff8, &[5, 0, 0, 0, 5, 0, 0, 0], ReadOnly)
            .memory(0x7000, &[5, 0, 0, 0, 5, 0, 0, 0, 5, 0, 0, 0], ReadOnly).fault(0x5000, 0),
        Case::preserving_flags("REPNE SCASW does not consume an incomplete current comparison", &[0xf2, 0x66, 0xaf])
            .stored_flags(record(0)).initial_register(Eax, 5).register(Ecx, 3, 2).register(Edi, 0x7ffd, 0x7fff)
            .memory(0x7ffd, &[3, 0, 3], ReadOnly).fault(0x8000, 0),
        Case::preserving_flags("REPE CMPSW destination failure leaves the current source index unchanged", &[0xf3, 0x66, 0xa7])
            .stored_flags(record(0)).register(Ecx, 3, 2).register(Esi, 0x4000, 0x4002).register(Edi, 0x7ffd, 0x7fff)
            .memory(0x4000, &[5, 0, 5, 0, 5, 0], ReadOnly).memory(0x7ffd, &[5, 0, 5], ReadOnly).fault(0x8000, 0),
        Case::preserving_flags("word repeated SCAS uses full ECX with default addressing", &[0xf3, 0x66, 0xaf])
            .stored_flags(record(0)).initial_register(Eax, 5).register(Ecx, 0x10000, 0xffff).register(Edi, 0x17ffe, 0x18000)
            .memory(0x17ffe, &[5, 0], ReadOnly).fault(0x18000, 0),
        Case::preserving_flags("conditional repeat restores entry flags on a segment fault after 16-bit progress", &[0x67, 0xf2, 0xae])
            .stored_flags(record(0)).segmented_only()
            .segment(Segment::Es, StoredSegment { base: 0x7000, limit: 0, ..StoredSegment::flat_data32(0x23) })
            .initial_register(Eax, 5).register(Ecx, 0xaaaa_0003, 0xaaaa_0002).register(Edi, 0xbbbb_0000, 0xbbbb_0001)
            .memory(0x7000, &[3], ReadOnly).general_protection(0),
        Case::preserving_flags("conditional CMPS SS source fault restores entry flags before accessing ES", &[0xf3, 0x36, 0xa6])
            .stored_flags(record(0)).segmented_only()
            .segment(Segment::Ss, StoredSegment { base: 0x4000, limit: 0, ..StoredSegment::flat_data32(0x23) })
            .register(Ecx, 3, 2).register(Esi, 0, 1).register(Edi, 0x7fff, 0x8000)
            .memory(0x4000, &[5], ReadOnly).memory(0x7fff, &[5], ReadOnly).stack_fault(0),
    ]
}

#[rustfmt::skip]
fn repeat_prefixes() -> Vec<Case> {
    vec![
        Case::replacing_flags("last F3 selects REPE after F2", &[0xf2, 0xf3, 0xae], flags(0))
            .stored_flags(record(0)).initial_register(Eax, 5).register(Ecx, 3, 1).register(Edi, 0x7000, 0x7002)
            .memory(0x7000, &[5, 3, 5], ReadOnly),
        Case::replacing_flags("last F2 selects REPNE after F3", &[0xf3, 0xf2, 0xae], flags(10))
            .stored_flags(record(0)).initial_register(Eax, 5).register(Ecx, 3, 2).register(Edi, 0x7000, 0x7001)
            .memory(0x7000, &[5, 3, 5], ReadOnly),
    ]
}

#[rustfmt::skip]
fn histories() -> Vec<Sequence> {
    let mut cases = Vec::new();
    for count in [0, 3] {
        let scan = if count == 0 { Step::preserving_flags(&[0xf2, 0xae]) } else {
            Step::new(&[0xf2, 0xae], flags(10)).register(Ecx, 1).register(Edi, 0x6002)
        };
        cases.push(Sequence::from_opaque_flags(format!("REPNE count {count} feeds SETZ and LAHF"))
            .stored_flags(record(0)).initial_registers(&[(Eax, 0x1122_007f), (Ebx, 0), (Ecx, count), (Edi, 0x6000)])
            .memory(0x6000, &[1, 0x80, 0x11], ReadOnly)
            .step(Step::new(&[0x04, 1], flags(52)).register(Eax, 0x1122_0080))
            .step(scan)
            .step(Step::preserving_flags(&[0x0f, 0x94, 0xc3]).register(Ebx, u32::from(count != 0)))
            .step(Step::preserving_flags(&[0x9f]).register(Eax, if count == 0 { 0x1122_9280 } else { 0x1122_4680 })));
    }
    for (prefix, opcode) in [(0xf3, 0xa6), (0xf2, 0xae)] {
        let mut fault = Step::preserving_flags(&[0x67, prefix, opcode])
            .register(Ecx, 0xaaaa_0001).register(Edi, 0xbbbb_8000).fault(0x8000, 0);
        if opcode == 0xa6 { fault = fault.register(Esi, 0xcccc_4002); }
        cases.push(Sequence::from_opaque_flags(format!("{prefix:02x} {opcode:02x} fault retains prior arithmetic STC and narrow writes"))
            .stored_flags(record(0)).initial_registers(&[(Eax, 0xabcd_3405), (Ebx, 0x7fff_ffff), (Ecx, 0xaaaa_dead),
                (Edi, 0xbbbb_7ffe), (Esi, 0xcccc_4000)])
            .memory(0x4000, &[5; 3], ReadOnly).memory(0x7ffe, &[if prefix == 0xf3 { 5 } else { 3 }; 2], ReadOnly)
            .step(Step::new(&[0x83, 0xc3, 1], flags(54)).register(Ebx, 0x8000_0000))
            .step(Step::new(&[0xf9], flags(55)))
            .step(Step::preserving_flags(&[0x66, 0xb9, 3, 0]).register(Ecx, 0xaaaa_0003))
            .step(Step::preserving_flags(&[0xb4, 0x12]).register(Eax, 0xabcd_1205))
            .step(fault));
    }
    for prefix in [0xf2, 0xf3] {
        let first = if prefix == 0xf3 { 3 } else { 5 };
        cases.push(Sequence::from_opaque_flags(format!("repeat prefix {prefix:02x} resets before the next SCAS"))
            .stored_flags(record(0)).initial_registers(&[(Eax, 5), (Ecx, 3), (Edi, 0x7000)])
            .memory(0x7000, &[first, 5, 5], ReadOnly)
            .step(Step::new(&[prefix, 0xae], flags(if prefix == 0xf3 { 0 } else { 10 }))
                .register(Ecx, 2).register(Edi, 0x7001))
            .step(Step::new(&[0xae], flags(10)).register(Edi, 0x7002)));
    }
    cases
}

test_cases!(
    conditions_count_direction_and_final_flags,
    stopping_conditions()
);
test_cases!(zero_count_skips_segments_and_preserves_flags, zero_count());
test_cases!(
    faults_restore_entry_flags_and_keep_progress,
    fault_rollback()
);
test_cases!(last_repeat_prefix_selects_the_condition, repeat_prefixes());
test_sequences!(prior_state_flag_consumers_and_prefix_reset, histories());
