//! XLAT replaces AL after a checked byte read through an address-sized table base.

use crate::support::{
    cases::{
        test_cases,
        FlagExpectation::{Clear, Set},
        Flags, InstructionCase as Case,
        Permissions::ReadOnly,
    },
    encoding::check_length,
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Sequence},
};
use wasm86_x86::{Gpr32::*, Segment, SegmentAttributes, StoredSegment};

#[rustfmt::skip]
fn byte_lookups() -> Vec<Case> {
    vec![
        Case::preserving_flags("zero index reads the table base and changes only AL", &[0xd7])
            .initial_register(Ebx, 0x4000).register(Eax, 0x9234_ab00, 0x9234_ab80)
            .memory(0x4000, &[0x80], ReadOnly),
        Case::preserving_flags("unsigned FF index reads exactly one byte at the page end", &[0xd7])
            .at(0x1fff).initial_register(Ebx, 0x4f00).register(Eax, 0xfedc_abff, 0xfedc_ab12)
            .memory(0x4fff, &[0x12], ReadOnly),
        Case::preserving_flags("operand override keeps byte data and a full EBX base", &[0x66, 0xd7])
            .initial_register(Ebx, 0x8000_4000).register(Eax, 0x9234_ab80, 0x9234_abe7)
            .memory(0x8000_4080, &[0xe7], ReadOnly),
        Case::preserving_flags("address32 addition wraps before the byte read", &[0xd7])
            .initial_register(Ebx, 0xffff_fff0).register(Eax, 0x9234_ab80, 0x9234_ab01)
            .memory(0x70, &[1], ReadOnly),
        Case::preserving_flags("address override ignores the upper EBX bits", &[0x67, 0xd7])
            .initial_register(Ebx, 0xdead_4000).register(Eax, 0x9234_ab7f, 0x9234_ab00)
            .memory(0x407f, &[0], ReadOnly),
    ]
}

fn code16_lookups() -> Vec<Case> {
    [
        Case::preserving_flags("16-bit code uses BX by default", &[0xd7])
            .initial_register(Ebx, 0xabcd_4080)
            .register(Eax, 0x9234_ab80, 0x9234_ab12)
            .memory(0x4100, &[0x12], ReadOnly),
        Case::preserving_flags(
            "67 selects EBX in 16-bit code independently of 66",
            &[0x66, 0x67, 0xd7],
        )
        .initial_register(Ebx, 0x8000_4000)
        .register(Eax, 0x9234_abff, 0x9234_ab80)
        .memory(0x8000_40ff, &[0x80], ReadOnly),
    ]
    .into_iter()
    .map(|case| {
        case.segmented_only().segment(
            Segment::Cs,
            StoredSegment {
                attributes: SegmentAttributes::from_bits(0x07),
                ..StoredSegment::flat_code32(0x1b)
            },
        )
    })
    .collect()
}

fn segments_and_faults() -> Vec<Case> {
    let table = StoredSegment {
        base: 0x8000,
        limit: 0x7f,
        ..StoredSegment::flat_data32(0x23)
    };
    vec![
        Case::preserving_flags(
            "address16 wraps before DS limit checking and base addition",
            &[0x67, 0xd7],
        )
        .segmented_only()
        .segment(
            Segment::Ds,
            StoredSegment {
                base: 0x10000,
                ..table
            },
        )
        .initial_register(Ebx, 0xabcd_fff0)
        .register(Eax, 0x9234_ab80, 0x9234_ab56)
        .memory(0x10070, &[0x56], ReadOnly),
        Case::preserving_flags("FS override replaces an unusable DS", &[0x64, 0xd7])
            .segmented_only()
            .segment(Segment::Ds, StoredSegment::unusable(0))
            .segment(Segment::Fs, table)
            .initial_register(Ebx, 0x60)
            .register(Eax, 0x9234_ab1f, 0x9234_ab78)
            .memory(0x807f, &[0x78], ReadOnly),
        Case::preserving_flags(
            "absent table byte preserves AL and reports a read fault",
            &[0xd7],
        )
        .initial_registers(&[(Ebx, 0x4000), (Eax, 0x9234_ab80)])
        .fault(0x4080, 0),
        Case::preserving_flags("unusable DS faults before replacing AL", &[0xd7])
            .segmented_only()
            .segment(Segment::Ds, StoredSegment::unusable(0))
            .initial_registers(&[(Ebx, 0x4000), (Eax, 0x9234_ab80)])
            .memory(0x4080, &[0x56], ReadOnly)
            .general_protection(0),
        Case::preserving_flags("DS checks the indexed byte against its limit", &[0xd7])
            .segmented_only()
            .segment(Segment::Ds, table)
            .initial_registers(&[(Ebx, 0x70), (Eax, 0x9234_ab10)])
            .memory(0x8080, &[0x56], ReadOnly)
            .general_protection(0),
        Case::preserving_flags(
            "SS override reports a stack segment limit fault",
            &[0x36, 0xd7],
        )
        .segmented_only()
        .segment(Segment::Ss, table)
        .initial_registers(&[(Ebx, 0x70), (Eax, 0x9234_ab10)])
        .memory(0x8080, &[0x56], ReadOnly)
        .stack_fault(0),
        Case::preserving_flags(
            "override page fault reports the translated linear address",
            &[0x64, 0xd7],
        )
        .segment(Segment::Fs, table)
        .initial_registers(&[(Ebx, 0x60), (Eax, 0x9234_ab1f)])
        .fault(0x807f, 0),
    ]
}

#[rustfmt::skip]
fn dependencies_and_faults() -> Vec<Sequence> {
    vec![
        Sequence::from_opaque_flags("XLAT chains AL results and preserves pending arithmetic flags")
            .initial_registers(&[(Eax, 0x9234_ab00), (Ebx, 0x4f20), (Ecx, 0)])
            .memory(0x4f7f, &[0x80, 0x81], ReadOnly)
            .step(Step::new(&[0x83, 0xe9, 1],
                Flags { cf: Set, pf: Set, af: Set, zf: Clear, sf: Set, of: Clear }).register(Ecx, 0xffff_ffff))
            .step(Step::preserving_flags(&[0xb3, 0]).register(Ebx, 0x4f00))
            .step(Step::preserving_flags(&[0xb0, 0x7f]).register(Eax, 0x9234_ab7f))
            .step(Step::preserving_flags(&[0xd7]).register(Eax, 0x9234_ab80))
            .step(Step::preserving_flags(&[0xd7]).register(Eax, 0x9234_ab81))
            .step(Step::preserving_flags(&[0x0f, 0x92, 0xc1]).register(Ecx, 0xffff_ff01)),
        Sequence::preserving_flags("a failed second lookup publishes only the completed first lookup")
            .initial_registers(&[(Eax, 0x9234_ab00), (Ebx, 0x4fff)])
            .memory(0x4fff, &[1], ReadOnly)
            .step(Step::preserving_flags(&[0xd7]).register(Eax, 0x9234_ab01))
            .step(Step::preserving_flags(&[0xd7]).fault(0x5000, 0))
            .trailing_code(&[0xb0, 0xff], 1),
    ]
}

#[test]
fn encoding_has_no_operand_bytes() {
    for code in [&[0xd7][..], &[0x66, 0xd7], &[0x64, 0x67, 0xd7]] {
        check_length(code);
    }
}

test_cases!(unsigned_index_and_byte_destination, byte_lookups());
test_cases!(code_default_and_address_override, code16_lookups());
test_cases!(segment_selection_and_read_faults, segments_and_faults());
test_sequences!(
    lookup_dependencies_and_fault_publication,
    dependencies_and_faults()
);
