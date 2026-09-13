use super::code16;
use crate::support::cases::{
    test_cases, InstructionCase as Case,
    Permissions::{ReadOnly, ReadWrite},
};
use wasm86_x86::{
    Gpr32::{Eax, Ebp, Ebx, Edi, Esi},
    Segment, StoredSegment,
};

fn layouts() -> Vec<Case> {
    let mut cases = Vec::new();
    // Literal expected offsets for BX=fff0, BP=20, SI=30 and DI=40.
    for (mode, displacement, offsets) in [
        (
            0,
            &[][..],
            [0x20, 0x30, 0x50, 0x60, 0x30, 0x40, 0x700, 0xfff0],
        ),
        (1, &[0xe0][..], [0, 0x10, 0x30, 0x40, 0x10, 0x20, 0, 0xffd0]),
        (
            2,
            &[0xf0, 0xff][..],
            [0x10, 0x20, 0x40, 0x50, 0x20, 0x30, 0x10, 0xffe0],
        ),
    ] {
        for (rm, offset) in offsets.into_iter().enumerate() {
            let displacement = if mode == 0 && rm == 6 {
                &[0, 7][..]
            } else {
                displacement
            };
            for default16 in [false, true] {
                let prefix = if default16 { 0x66 } else { 0x67 };
                let code = [&[prefix, 0x8d, (mode << 6) | rm as u8][..], displacement].concat();
                let case = Case::preserving_flags(
                    format!("16-bit LEA mode={mode}, rm={rm}, CS.D16={default16}"),
                    &code,
                )
                .initial_registers(&[
                    (Ebx, 0xaaaa_fff0),
                    (Ebp, 0xbbbb_0020),
                    (Esi, 0xcccc_0030),
                    (Edi, 0xdddd_0040),
                ])
                .register(Eax, 0xdead_beef, offset);
                cases.push(if default16 { code16(case) } else { case });
            }
        }
    }
    cases
}

fn segments_and_fields() -> Vec<Case> {
    let mut cases = Vec::new();
    for (rm, offset, stack) in [
        (0, 0x20, false),
        (2, 0x50, true),
        (4, 0x30, false),
        (6, 0x700, false),
        (7, 0xfff0, false),
    ] {
        for override_fs in [false, true] {
            let mut code = vec![0x67];
            if override_fs {
                code.push(0x64);
            }
            code.extend([0x8b, rm]);
            if rm == 6 {
                code.extend([0, 7]);
            }
            let base = if override_fs {
                0x10000
            } else if stack {
                0x8000
            } else {
                0x4000
            };
            cases.push(
                Case::preserving_flags(
                    format!("16-bit memory rm={rm}, override={override_fs}"),
                    &code,
                )
                .segmented_only()
                .segment(
                    Segment::Ds,
                    StoredSegment {
                        base: 0x4000,
                        ..StoredSegment::flat_data32(0)
                    },
                )
                .segment(
                    Segment::Ss,
                    StoredSegment {
                        base: 0x8000,
                        ..StoredSegment::flat_data32(0)
                    },
                )
                .segment(
                    Segment::Fs,
                    StoredSegment {
                        base: 0x10000,
                        ..StoredSegment::flat_data32(0)
                    },
                )
                .initial_registers(&[(Ebx, 0xaaaa_fff0), (Ebp, 0xbbbb_0020), (Esi, 0xcccc_0030)])
                .register(Eax, 0, 0x1234_5678)
                .memory(base + offset, &[0x78, 0x56, 0x34, 0x12], ReadOnly),
            );
        }
    }
    cases.extend([
        Case::preserving_flags(
            "disp16 ends before the immediate",
            &[0x67, 0xc7, 0x80, 0xf0, 0xff, 0x78, 0x56, 0x34, 0x12],
        )
        .initial_registers(&[(Ebx, 0x4000), (Esi, 0x20)])
        .memory(0x4010, &[0xff; 4], ReadWrite)
        .expect_memory(0x4010, &[0x78, 0x56, 0x34, 0x12]),
        Case::preserving_flags(
            "absolute disp16 ends before the immediate",
            &[0x67, 0xc7, 0x06, 0, 0x40, 0x78, 0x56, 0x34, 0x12],
        )
        .memory(0x4000, &[0xff; 4], ReadWrite)
        .expect_memory(0x4000, &[0x78, 0x56, 0x34, 0x12]),
        Case::preserving_flags(
            "address width does not wrap individual bytes",
            &[0x67, 0xa1, 0xff, 0xff],
        )
        .register(Eax, 0, 0x1234_5678)
        .memory(0xffff, &[0x78, 0x56, 0x34, 0x12], ReadOnly),
        Case::preserving_flags(
            "word operand and 16-bit offset are independent",
            &[0x66, 0x67, 0xa1, 0, 0x40],
        )
        .register(Eax, 0xaaaa_0000, 0xaaaa_5678)
        .memory(0x4000, &[0x78, 0x56], ReadOnly),
        code16(
            Case::preserving_flags(
                "67 selects a 32-bit SIB address in 16-bit code",
                &[0x67, 0x8b, 0x44, 0xb3, 0xf0],
            )
            .initial_registers(&[(Ebx, 0x14000), (Esi, 8)])
            .register(Eax, 0xaaaa_0000, 0xaaaa_5678)
            .memory(0x14010, &[0x78, 0x56], ReadOnly),
        ),
    ]);
    cases
}

test_cases!(all_16bit_modrm_layouts_and_displacements, layouts());
test_cases!(
    segment_selection_and_field_boundaries,
    segments_and_fields()
);

fn bit_string_offsets() -> Vec<Case> {
    use crate::support::cases::{
        FlagExpectation::{Clear, Preserved, Set, Undefined},
        Flags,
    };
    use wasm86_x86::Gpr32::Ecx;
    let flags = |cf| Flags {
        cf,
        zf: Preserved,
        pf: Undefined,
        af: Undefined,
        sf: Undefined,
        of: Undefined,
    };
    vec![
        Case::new(
            "negative bit index wraps a 16-bit effective address",
            &[0x67, 0x0f, 0xa3, 0x0e, 0, 0],
            Flags::all(false),
            flags(Set),
        )
        .initial_register(Ecx, u32::MAX)
        .memory(0xfffc, &[0, 0, 0, 0x80], ReadOnly),
        Case::new(
            "BTS wraps the adjusted BP address before SS translation",
            &[0x66, 0x67, 0x0f, 0xab, 0x4e, 0],
            Flags::all(false),
            flags(Clear),
        )
        .segmented_only()
        .segment(
            Segment::Ss,
            StoredSegment {
                base: 0x8000,
                ..StoredSegment::flat_data32(0)
            },
        )
        .segment(Segment::Ds, StoredSegment::unusable(0))
        .initial_registers(&[(Ebp, 0xabcd_fffe), (Ecx, 16)])
        .memory(0x8000, &[0, 0], ReadWrite)
        .expect_memory(0x8000, &[1, 0]),
        Case::new(
            "BTR wraps a positive dword bit-unit offset",
            &[0x67, 0x0f, 0xb3, 0x0e, 0xfc, 0xff],
            Flags::all(false),
            flags(Set),
        )
        .initial_register(Ecx, 32)
        .memory(0, &[1, 0, 0, 0], ReadWrite)
        .expect_memory(0, &[0; 4]),
        Case::new(
            "BTC wraps a negative word bit-unit offset",
            &[0x66, 0x67, 0x0f, 0xbb, 0x0e, 0, 0],
            Flags::all(false),
            flags(Set),
        )
        .initial_register(Ecx, 0xffff)
        .memory(0xfffe, &[0, 0x80], ReadWrite)
        .expect_memory(0xfffe, &[0; 2]),
        Case::preserving_flags(
            "adjusted bit-string span faults before flags or memory change",
            &[0x66, 0x67, 0x0f, 0xab, 0x4e, 0],
        )
        .segmented_only()
        .segment(
            Segment::Ss,
            StoredSegment {
                base: 0x8000,
                limit: 0,
                ..StoredSegment::flat_data32(0)
            },
        )
        .initial_registers(&[(Ebp, 0xabcd_fffe), (Ecx, 16)])
        .memory(0x8000, &[0xff; 2], ReadWrite)
        .stack_fault(0),
    ]
}

test_cases!(
    bit_string_adjustments_precede_address_wrapping_and_span_checks,
    bit_string_offsets()
);
