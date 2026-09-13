use super::{code, data};
use crate::support::cases::{test_cases, InstructionCase as Case, Permissions::ReadOnly};
use wasm86_x86::{
    Gpr32::{Eax, Ebp, Ebx},
    Segment, SegmentAttributes,
};

fn translated_fields() -> Vec<Case> {
    let mut cases = Vec::new();
    for base in [0x6003, 0xffff_f003] {
        for eip in [0x20, 0xfff, 0xffff_fffd] {
            for (bytes, value) in [
                (&[0xb8, 0x78, 0x56, 0x34, 0x12][..], 0x1234_5678),
                (&[0x66, 0xb8, 0x78, 0x56], 0xaaaa_5678),
                (&[0xa1, 0x00, 0x40, 0, 0], 0x1234_5678),
                (&[0x8b, 0x83, 0x10, 0, 0, 0], 0x1234_5678),
                (&[0x8b, 0x04, 0xad, 0xf0, 0x3f, 0, 0], 0x1234_5678),
                (&[0x0f, 0xb6, 0x43, 0x10], 0x78),
                (&[0x64, 0x66, 0x0f, 0xb7, 0x43, 0x10], 0xaaaa_1234),
                (&[0x66, 0x65, 0x64, 0x8b, 0x43, 0x10], 0xaaaa_1234),
            ] {
                cases.push(
                    Case::preserving_flags(
                        format!("CS base {base:08x}, EIP {eip:08x}, fields {bytes:02x?}"),
                        bytes,
                    )
                    .segmented_only()
                    .at(eip)
                    .segment(Segment::Cs, code(base, u32::MAX))
                    .segment(Segment::Fs, data(4, u32::MAX))
                    .segment(Segment::Gs, data(8, u32::MAX))
                    .initial_register(Ebx, 0x3ff0)
                    .initial_register(Ebp, 4)
                    .register(Eax, 0xaaaa_bbbb, value)
                    .memory(
                        0x4000,
                        &[0x78, 0x56, 0x34, 0x12, 0x34, 0x12, 0, 0, 0xff, 0xff],
                        ReadOnly,
                    ),
                );
            }
        }
    }
    cases.push(
        Case::preserving_flags(
            "CS-translated immediate crosses scattered code pages",
            &[0xb8, 0x78, 0x56, 0x34, 0x12],
        )
        .at(0xfeb)
        .segmented_only()
        .segment(Segment::Cs, code(0x6013, 0x1000))
        .map_page(6, 0xc000, ReadOnly)
        .map_page(7, 0x3000, ReadOnly)
        .register(Eax, 0, 0x1234_5678),
    );
    for readable in [false, true] {
        let mut cs = code(0x8000, 0x1002);
        cs.attributes = SegmentAttributes::from_bits(if readable { 0x17 } else { 0x13 });
        let case = Case::preserving_flags(
            format!("CS override reads through the code base, readable {readable}"),
            &[0x2e, 0x8b, 0x03],
        )
        .segmented_only()
        .segment(Segment::Cs, cs)
        .initial_register(Ebx, 0x20)
        .register(Eax, 0, if readable { 0x1234_5678 } else { 0 })
        .memory(0x8020, &[0x78, 0x56, 0x34, 0x12], ReadOnly);
        cases.push(if readable {
            case
        } else {
            case.general_protection(0)
        });
    }
    cases
}

test_cases!(
    cs_base_translates_all_instruction_fields_and_wrapping_offsets,
    translated_fields()
);

fn code_limits() -> Vec<Case> {
    let mut cases = Vec::new();
    for bytes in [
        vec![0x90],
        vec![0xb8, 0x78, 0x56, 0x34, 0x12],
        vec![0x66, 0xb8, 0x78, 0x56],
        vec![0x64, 0x66, 0x0f, 0xb7, 0x43, 0x10],
        [vec![0x64; 12], vec![0x66, 0x89, 0xc0]].concat(),
    ] {
        let last = 0x1000 + bytes.len() as u32 - 1;
        let value = if bytes.contains(&0xb8) {
            if bytes[0] == 0x66 {
                0xaaaa_5678
            } else {
                0x1234_5678
            }
        } else if bytes.contains(&0xb7) {
            0xaaaa_5678
        } else {
            0xaaaa_bbbb
        };
        let fits = Case::preserving_flags(
            format!("instruction ends exactly at CS limit {bytes:02x?}"),
            &bytes,
        )
        .segmented_only()
        .segment(Segment::Cs, code(0x8000, last))
        .register(Eax, 0xaaaa_bbbb, value)
        .initial_register(Ebx, 0x3ff0)
        .memory(0x4000, &[0x78, 0x56], ReadOnly);
        cases.push(fits);
        cases.push(
            Case::preserving_flags(
                format!("required byte beyond CS limit {bytes:02x?}"),
                &bytes,
            )
            .segmented_only()
            .segment(Segment::Cs, code(0x8000, last - 1))
            .initial_register(Eax, 0xaaaa_bbbb)
            .initial_register(Ebx, 0x3ff0)
            .memory(0x4000, &[0x78, 0x56], ReadOnly)
            .general_protection(0),
        );
    }
    for attributes in [0x10, 0x15, 0x1b, 0xffff] {
        let mut cs = code(0x8000, 0x1000);
        cs.attributes = SegmentAttributes::from_bits(attributes);
        cases.push(
            Case::preserving_flags(
                format!("CS attributes {attributes:04x} forbid fetch"),
                &[0x90],
            )
            .segmented_only()
            .segment(Segment::Cs, cs)
            .general_protection(0),
        );
    }
    let mut cs = code(0x8000, 0x1000);
    cs.attributes = SegmentAttributes::from_bits(0x13);
    cases.push(
        Case::preserving_flags("execute-only nonflat CS permits its final byte", &[0x90])
            .segmented_only()
            .segment(Segment::Cs, cs),
    );
    cases
}

test_cases!(
    instruction_fetch_checks_permissions_and_only_required_bytes,
    code_limits()
);
