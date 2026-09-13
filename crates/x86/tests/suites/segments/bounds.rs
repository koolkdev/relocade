use super::data;
use crate::support::cases::{
    test_cases, InstructionCase as Case,
    Permissions::{ReadOnly, ReadWrite},
};
use wasm86_x86::{
    Gpr32::{Eax, Ebx},
    Segment, SegmentAttributes, StoredSegment,
};

fn finite_spans() -> Vec<Case> {
    let mut cases = vec![Case::preserving_flags(
        "B=0 does not shorten an expand-up data segment",
        &[0x64, 0x8b, 0x03],
    )
    .segment(
        Segment::Fs,
        StoredSegment {
            attributes: SegmentAttributes::from_bits(0x05),
            ..data(0x8000, 0x1ffff)
        },
    )
    .initial_register(Ebx, 0x10000)
    .register(Eax, 0, 0x1234_5678)
    .memory(0x18000, &[0x78, 0x56, 0x34, 0x12], ReadOnly)];
    for (code, width, value) in [
        (&[0x64, 0x8a, 0x03][..], 1, 0xa5a5_a578),
        (&[0x64, 0x66, 0x8b, 0x03], 2, 0xa5a5_5678),
        (&[0x64, 0x8b, 0x03], 4, 0x1234_5678),
    ] {
        for (offset, allowed) in [(0x21 - width, true), (0x22 - width, false), (0x21, false)] {
            let case = Case::preserving_flags(
                format!("{width}-byte operand at {offset:02x} with limit 20"),
                code,
            )
            .segment(Segment::Fs, data(0x8000, 0x20))
            .initial_register(Ebx, offset)
            .memory(0x8000 + offset, &[0x78, 0x56, 0x34, 0x12], ReadOnly);
            cases.push(if allowed {
                case.register(Eax, 0xa5a5_a5a5, value)
            } else {
                case.general_protection(0)
            });
        }
    }
    cases.push(
        Case::preserving_flags("a zero limit admits its one byte", &[0x64, 0x8a, 0x03])
            .segment(Segment::Fs, data(0x8000, 0))
            .initial_register(Ebx, 0)
            .register(Eax, 0x1122_3344, 0x1122_3378)
            .memory(0x8000, &[0x78], ReadOnly),
    );
    for limit in [0, 1, 2, u32::MAX - 1] {
        let offset = if limit == u32::MAX - 1 { limit } else { 0 };
        cases.push(
            Case::preserving_flags(
                format!("dword does not fit limit {limit:08x}"),
                &[0x64, 0x8b, 0x03],
            )
            .segment(Segment::Fs, data(0, limit))
            .initial_register(Ebx, offset)
            .general_protection(0),
        );
    }
    cases.push(
        Case::preserving_flags(
            "bit-string adjustment is checked as an offset",
            &[0x64, 0x0f, 0xa3, 0x0b],
        )
        .segment(Segment::Fs, data(0x8000, 0x23))
        .initial_registers(&[(Ebx, 0x20), (wasm86_x86::Gpr32::Ecx, 32)])
        .general_protection(0),
    );
    cases
}

fn expand_down() -> Vec<Case> {
    let mut cases = Vec::new();
    // D/B controls the upper offset bound, independent of the address size.
    for (bits, limit, offset, allowed) in [
        (0x0d, 0xff, 0xff, false),
        (0x0d, 0xff, 0x100, true),
        (0x0d, 0xff, 0xfffe, true),
        (0x0d, 0xff, 0xffff, false),
        (0x0d, 0xff, 0x1_0000, false),
        (0x0d, 0xffff, 0xffff, false),
        (0x1d, 0xff, 0x1_0000, true),
        (0x1d, 0xff, 0xffff_fffe, true),
        (0x1d, 0xff, 0xffff_ffff, false),
        (0x1d, u32::MAX, 0, false),
    ] {
        let case = Case::preserving_flags(
            format!("expand-down {bits:02x}, limit {limit:08x}, offset {offset:08x}"),
            &[0x64, 0x66, 0x8b, 0x03],
        )
        .segment(
            Segment::Fs,
            StoredSegment {
                attributes: SegmentAttributes::from_bits(bits),
                ..data(0, limit)
            },
        )
        .initial_register(Ebx, offset);
        cases.push(if allowed {
            case.memory(offset, &[0x78, 0x56], ReadOnly)
                .register(Eax, 0xabcd_0000, 0xabcd_5678)
        } else {
            case.general_protection(0)
        });
    }
    cases
}

fn linear_wrap() -> Vec<Case> {
    let mut cases = Vec::new();
    for (base, offset, limit) in [(0xffff_fff0, 0xf, 0xff), (0, u32::MAX, u32::MAX)] {
        for (code, width, value) in [
            (&[0x64, 0x66, 0x8b, 0x03][..], 2, 0xabcd_5678),
            (&[0x64, 0x8b, 0x03], 4, 0x1234_5678),
        ] {
            cases.push(
                Case::preserving_flags(
                    format!("linear wrap reads {width} bytes, base {base:08x}"),
                    code,
                )
                .segment(Segment::Fs, data(base, limit))
                .initial_register(Ebx, offset)
                .register(Eax, 0xabcd_0000, value)
                .map_page(0xfffff, 0x8000, ReadOnly)
                .map_page(0, 0xa000, ReadOnly)
                .backing(0x8fff, &[0x78])
                .backing(0xa000, &[0x56, 0x34, 0x12]),
            );
        }
        cases.push(
            Case::preserving_flags(
                format!("linear wrap writes a dword, base {base:08x}"),
                &[0x64, 0x89, 0x03],
            )
            .segment(Segment::Fs, data(base, limit))
            .initial_registers(&[(Ebx, offset), (Eax, 0x1234_5678)])
            .map_page(0xfffff, 0x8000, ReadWrite)
            .map_page(0, 0xa000, ReadWrite)
            .backing(0x8fff, &[0xff])
            .backing(0xa000, &[0xff; 3])
            .expect_memory(u32::MAX, &[0x78])
            .expect_memory(0, &[0x56, 0x34, 0x12]),
        );
    }
    for (code, first, second, address, error) in [
        (&[0x64, 0x8b, 0x03][..], None, None, u32::MAX, 0),
        (&[0x64, 0x8b, 0x03], Some(ReadOnly), None, 0, 0),
        (&[0x64, 0x89, 0x03], Some(ReadOnly), None, u32::MAX, 3),
        (&[0x64, 0x89, 0x03], Some(ReadWrite), None, 0, 2),
        (&[0x64, 0x89, 0x03], Some(ReadWrite), Some(ReadOnly), 0, 3),
    ] {
        let mut case = Case::preserving_flags(
            format!("linear wrap faults at {address:08x}, error {error}"),
            code,
        )
        .segment(Segment::Fs, data(0xffff_fff0, 0xff))
        .initial_register(Ebx, 0xf)
        .backing(0x8fff, &[0x78])
        .backing(0xa000, &[0x56, 0x34, 0x12])
        .fault(address, error);
        if let Some(permissions) = first {
            case = case.map_page(0xfffff, 0x8000, permissions);
        }
        if let Some(permissions) = second {
            case = case.map_page(0, 0xa000, permissions);
        }
        cases.push(case);
    }
    cases
}

test_cases!(complete_offsets_must_fit_finite_limits, finite_spans());
test_cases!(
    expand_down_uses_exclusive_lower_and_sized_upper_bounds,
    expand_down()
);
test_cases!(
    linear_address_wrap_uses_both_real_page_mappings,
    linear_wrap()
);
