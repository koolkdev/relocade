//! REP LODS carries the last completed accumulator through success and faults.

use super::{flags, record};
use crate::support::{
    cases::{test_cases, InstructionCase as Case, Permissions::ReadOnly},
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Sequence},
};
use wasm86_x86::{Gpr32::*, Segment, SegmentAttributes, StoredSegment};

#[rustfmt::skip]
fn loads() -> Vec<Case> {
    vec![
        Case::preserving_flags("REP LODSB leaves the last byte and preserves upper EAX", &[0xf3, 0xac])
            .stored_flags(record(0xfe)).register(Eax, 0xaabb_ccdd, 0xaabb_cc56)
            .register(Ecx, 3, 0).register(Esi, 0x4000, 0x4003)
            .memory(0x4000, &[0x12, 0x34, 0x56], ReadOnly),
        Case::preserving_flags("REP LODSW loads backward into AX", &[0xf3, 0x66, 0xad])
            .stored_flags(record(0xff)).register(Eax, 0xaabb_ccdd, 0xaabb_3412)
            .register(Ecx, 2, 0).register(Esi, 0x4002, 0x3ffe)
            .memory(0x4000, &[0x12, 0x34, 0x56, 0x78], ReadOnly),
        Case::preserving_flags("REP LODSD stops after the final mapped element", &[0xf3, 0xad])
            .stored_flags(record(0)).register(Eax, 0xaabb_ccdd, 0x7856_3412)
            .register(Ecx, 1, 0).register(Esi, 0x4ffc, 0x5000)
            .memory(0x4ffc, &[0x12, 0x34, 0x56, 0x78], ReadOnly),
        Case::preserving_flags("16-bit REP LODSW uses CX and wraps SI between loads", &[0xf3, 0xad])
            .stored_flags(record(0)).segmented_only()
            .segment(Segment::Cs, StoredSegment { attributes: SegmentAttributes::from_bits(0x07), ..StoredSegment::flat_code32(0x1b) })
            .register(Eax, 0xaabb_ccdd, 0xaabb_7856)
            .register(Ecx, 0xcccc_0002, 0xcccc_0000).register(Esi, 0xdddd_fffe, 0xdddd_0002)
            .memory(0xfffe, &[0x12, 0x34], ReadOnly).memory(0, &[0x56, 0x78], ReadOnly),
        Case::preserving_flags("size overrides load dwords with full ESI in 16-bit code", &[0xf3, 0x66, 0x67, 0xad])
            .stored_flags(record(1)).segmented_only()
            .segment(Segment::Cs, StoredSegment { attributes: SegmentAttributes::from_bits(0x07), ..StoredSegment::flat_code32(0x1b) })
            .register(Eax, 0, 0x4433_2211).register(Ecx, 2, 0).register(Esi, 0x14004, 0x13ffc)
            .memory(0x14000, &[0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88], ReadOnly),
        Case::preserving_flags("REP LODS uses the last source override", &[0xf3, 0x65, 0x64, 0xac])
            .stored_flags(record(0)).segment(Segment::Gs, StoredSegment::unusable(0))
            .segment(Segment::Fs, StoredSegment { base: 0x4000, ..StoredSegment::flat_data32(0x23) })
            .register(Eax, 0xaabb_ccdd, 0xaabb_cc34).register(Ecx, 2, 0).register(Esi, 0, 2)
            .memory(0x4000, &[0x12, 0x34], ReadOnly),
    ]
}

fn zero_count() -> Vec<Case> {
    [vec![0xac], vec![0x66, 0xad], vec![0xad]]
        .into_iter()
        .map(|suffix| {
            let code = [vec![0xf3, 0x67, 0x64], suffix].concat();
            Case::preserving_flags("zero CX skips an unusable source and preserves EAX", &code)
                .stored_flags(record(0xff))
                .segment(Segment::Fs, StoredSegment::unusable(0))
                .initial_registers(&[(Eax, 0xaabb_ccdd), (Ecx, 0xcccc_0000), (Esi, u32::MAX)])
        })
        .collect()
}

#[rustfmt::skip]
fn faults() -> Vec<Case> {
    vec![
        Case::preserving_flags("first REP LODSB fault retains the entry accumulator", &[0xf3, 0xac])
            .stored_flags(record(0)).initial_registers(&[(Eax, 0xaabb_ccdd), (Ecx, 3), (Esi, 0x5000)])
            .fault(0x5000, 0),
        Case::preserving_flags("late REP LODSB fault retains the second loaded byte", &[0xf3, 0xac])
            .stored_flags(record(0)).register(Eax, 0xaabb_ccdd, 0xaabb_cc34)
            .register(Ecx, 3, 1).register(Esi, 0x4ffe, 0x5000)
            .memory(0x4ffe, &[0x12, 0x34], ReadOnly).fault(0x5000, 0),
        Case::preserving_flags("split REP LODSW fault cannot replace the previous complete word", &[0xf3, 0x66, 0xad])
            .stored_flags(record(0xfe)).register(Eax, 0xaabb_ccdd, 0xaabb_3412)
            .register(Ecx, 3, 2).register(Esi, 0x4ffd, 0x4fff)
            .memory(0x4ffd, &[0x12, 0x34, 0x56], ReadOnly).fault(0x5000, 0),
        Case::preserving_flags("REP LODSD uses full ECX and retains a completed dword", &[0xf3, 0xad])
            .stored_flags(record(0)).register(Eax, 0xaabb_ccdd, 0x7856_3412)
            .register(Ecx, 0x10000, 0xffff).register(Esi, 0x4ffc, 0x5000)
            .memory(0x4ffc, &[0x12, 0x34, 0x56, 0x78], ReadOnly).fault(0x5000, 0),
        Case::preserving_flags("backward REP LODSW retains a load before an SS limit fault", &[0xf3, 0x36, 0x66, 0xad])
            .stored_flags(record(0xff)).segmented_only()
            .segment(Segment::Ss, StoredSegment { base: 0x4000, limit: 2, ..StoredSegment::flat_data32(0x23) })
            .register(Eax, 0xaabb_ccdd, 0xaabb_3412).register(Ecx, 2, 1).register(Esi, 1, u32::MAX)
            .memory(0x4001, &[0x12, 0x34], ReadOnly).stack_fault(0),
    ]
}

#[rustfmt::skip]
fn histories() -> Vec<Sequence> {
    vec![
        Sequence::from_opaque_flags("REP LODSB keeps pending ADD flags and AH on a later fault")
            .stored_flags(record(0)).instruction_count(u32::MAX - 1)
            .initial_registers(&[(Eax, 0xaabb_cc7f), (Ecx, 0xdddd_00aa), (Esi, 0xeeee_4ffe)])
            .memory(0x4ffe, &[0x12, 0x34], ReadOnly)
            .step(Step::new(&[0x04, 1], flags(52)).register(Eax, 0xaabb_cc80))
            .step(Step::preserving_flags(&[0xb4, 0x56]).register(Eax, 0xaabb_5680))
            .step(Step::preserving_flags(&[0x66, 0xb9, 3, 0]).register(Ecx, 0xdddd_0003))
            .step(Step::preserving_flags(&[0xf3, 0x67, 0xac]).register(Eax, 0xaabb_5634)
                .register(Ecx, 0xdddd_0001).register(Esi, 0xeeee_5000).fault(0x5000, 0)),
        Sequence::preserving_flags("REP LODSW feeds a full accumulator read and resets prefixes before LODSD")
            .stored_flags(record(0)).initial_registers(&[(Eax, 0xaabb_ccdd), (Ecx, 2), (Esi, 0x4000)])
            .memory(0x4000, &[0x12, 0x34, 0x56, 0x78, 0x11, 0x22, 0x33, 0x44], ReadOnly)
            .step(Step::preserving_flags(&[0xf3, 0x66, 0xad]).register(Eax, 0xaabb_7856)
                .register(Ecx, 0).register(Esi, 0x4004))
            .step(Step::preserving_flags(&[0x89, 0xc3]).register(Ebx, 0xaabb_7856))
            .step(Step::preserving_flags(&[0xad]).register(Eax, 0x4433_2211).register(Esi, 0x4008)),
    ]
}

test_cases!(completed_loads, loads());
test_cases!(zero_count_preserves_accumulator, zero_count());
test_cases!(faults_keep_last_complete_load, faults());
test_sequences!(accumulator_progress_and_successors, histories());
