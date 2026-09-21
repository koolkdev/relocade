use super::super::data;
use crate::support::cases::{test_cases, InstructionCase as Case, Permissions::ReadWrite};
use crate::{Gpr32::*, Segment, SegmentAttributes, StoredSegment};

fn stores() -> Vec<Case> {
    let mut cases = Vec::new();
    for (segment, code, destination, output, code16) in [
        (Segment::Es, &[0x8c, 0xc0][..], Eax, 0xf327, false),
        (
            Segment::Cs,
            &[0x66, 0x8c, 0xc9][..],
            Ecx,
            0xaaaa_f327,
            false,
        ),
        (Segment::Ss, &[0x8c, 0xd2][..], Edx, 0xaaaa_f327, true),
        (Segment::Ds, &[0x66, 0x8c, 0xdb][..], Ebx, 0xf327, true),
        (
            Segment::Fs,
            &[0x66, 0x8c, 0xe4][..],
            Esp,
            0xaaaa_f327,
            false,
        ),
        (Segment::Gs, &[0x8c, 0xef][..], Edi, 0xf327, false),
    ] {
        let cache = if segment == Segment::Cs {
            StoredSegment::flat_code32(0xf327)
        } else {
            StoredSegment::flat_data32(0xf327)
        };
        let mut case =
            Case::preserving_flags(format!("MOV {destination:?},{segment:?} {code:02x?}"), code)
                .segment(segment, cache)
                .register(destination, 0xaaaa_5555, output);
        if code16 {
            case = case.segmented_only().segment(
                Segment::Cs,
                StoredSegment {
                    attributes: SegmentAttributes::from_bits(0x07),
                    ..StoredSegment::flat_code32(0x1b)
                },
            );
        }
        cases.push(case);
    }
    cases.extend([
        Case::preserving_flags(
            "MOV reads a null visible selector from an unusable DS",
            &[0x8c, 0xd8],
        )
        .segmented_only()
        .segment(Segment::Ds, StoredSegment::unusable(3))
        .register(Eax, 0x1234_5678, 3),
        Case::preserving_flags(
            "MOV reads a nonnull visible selector from an unusable FS",
            &[0x8c, 0xe0],
        )
        .segment(Segment::Fs, StoredSegment::unusable(0xf327))
        .register(Eax, 0x1234_5678, 0xf327),
        Case::preserving_flags(
            "selector store writes only two bytes through the last override",
            &[0x3e, 0x65, 0x67, 0x8c, 0x07],
        )
        .initial_register(Ebx, 0xabcd_0ffe)
        .segment(Segment::Gs, data(0x4000, 0xfff))
        .segment(Segment::Es, StoredSegment::flat_data32(0xf327))
        .memory(0x4ffd, &[0xaa, 0xbb, 0xcc], ReadWrite)
        .expect_memory(0x4ffe, &[0x27, 0xf3]),
        Case::preserving_flags(
            "dword operand override still stores a word in 16-bit code",
            &[0x65, 0x67, 0x66, 0x8c, 0x03],
        )
        .segmented_only()
        .segment(
            Segment::Cs,
            StoredSegment {
                attributes: SegmentAttributes::from_bits(0x07),
                ..StoredSegment::flat_code32(0x1b)
            },
        )
        .initial_register(Ebx, 0xffe)
        .segment(Segment::Gs, data(0x4000, 0xfff))
        .segment(Segment::Es, StoredSegment::flat_data32(0xf327))
        .memory(0x4ffd, &[0xaa, 0xbb, 0xcc], ReadWrite)
        .expect_memory(0x4ffe, &[0x27, 0xf3]),
    ]);
    cases
}

test_cases!(visible_selectors_and_destination_widths, stores());
