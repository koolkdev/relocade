use std::mem::{offset_of, size_of};

use crate::{CpuState, Segment, SegmentAttributes, Segments, StoredSegment};

#[test]
fn segment_records_fill_the_reserved_slots_without_moving_other_fields() {
    assert_eq!(size_of::<SegmentAttributes>(), 2);
    assert_eq!(size_of::<StoredSegment>(), 12);
    assert_eq!(size_of::<Segments>(), 72);
    assert_eq!(
        [
            offset_of!(StoredSegment, base),
            offset_of!(StoredSegment, limit),
            offset_of!(StoredSegment, selector),
            offset_of!(StoredSegment, attributes),
        ],
        [0, 4, 8, 10]
    );
    assert_eq!(
        [
            offset_of!(CpuState, segments.es),
            offset_of!(CpuState, segments.cs),
            offset_of!(CpuState, segments.ss),
            offset_of!(CpuState, segments.ds),
            offset_of!(CpuState, segments.fs),
            offset_of!(CpuState, segments.gs),
        ],
        [60, 72, 84, 96, 108, 120]
    );
}

#[test]
fn decoding_retains_every_segment_field_and_unknown_attribute_bit() {
    let bytes = std::array::from_fn(|index| index as u8);
    let cpu = CpuState::from_bytes(bytes);
    let expected = [
        (Segment::Es, 0x3f3e_3d3c, 0x4342_4140, 0x4544, 0x4746),
        (Segment::Cs, 0x4b4a_4948, 0x4f4e_4d4c, 0x5150, 0x5352),
        (Segment::Ss, 0x5756_5554, 0x5b5a_5958, 0x5d5c, 0x5f5e),
        (Segment::Ds, 0x6362_6160, 0x6766_6564, 0x6968, 0x6b6a),
        (Segment::Fs, 0x6f6e_6d6c, 0x7372_7170, 0x7574, 0x7776),
        (Segment::Gs, 0x7b7a_7978, 0x7f7e_7d7c, 0x8180, 0x8382),
    ];
    for (segment, base, limit, selector, attributes) in expected {
        assert_eq!(
            cpu.segments[segment],
            StoredSegment {
                base,
                limit,
                selector,
                attributes: SegmentAttributes::from_bits(attributes)
            },
            "{segment:?}"
        );
    }
    assert_eq!(cpu.to_bytes(), bytes);
}

#[test]
fn encoding_a_named_segment_changes_only_its_twelve_bytes() {
    for (segment, offset) in [
        (Segment::Es, 60),
        (Segment::Cs, 72),
        (Segment::Ss, 84),
        (Segment::Ds, 96),
        (Segment::Fs, 108),
        (Segment::Gs, 120),
    ] {
        let mut cpu = CpuState::filled(0xa5);
        cpu.segments[segment] = StoredSegment {
            base: 0x1234_5678,
            limit: 0xfedc_ba98,
            selector: 0x1357,
            attributes: SegmentAttributes::from_bits(0xbeef),
        };
        let mut expected = [0xa5; CpuState::BYTE_LEN];
        expected[offset..offset + 12].copy_from_slice(&[
            0x78, 0x56, 0x34, 0x12, 0x98, 0xba, 0xdc, 0xfe, 0x57, 0x13, 0xef, 0xbe,
        ]);
        assert_eq!(cpu.to_bytes(), expected, "{segment:?}");
        assert_eq!(CpuState::from_bytes(expected), cpu);
    }
}

#[test]
fn cpu_default_installs_flat_caches_while_literal_zero_images_stay_zero() {
    let cpu = CpuState::default();
    let mut expected = [0; 152];
    for (offset, attributes) in [
        (60, 0x15),
        (72, 0x17),
        (84, 0x15),
        (96, 0x15),
        (108, 0x15),
        (120, 0x15),
    ] {
        expected[offset..offset + 12]
            .copy_from_slice(&[0, 0, 0, 0, 0xff, 0xff, 0xff, 0xff, 0, 0, attributes, 0]);
    }
    assert_eq!(&cpu.to_bytes()[..152], &expected);
    assert_eq!(cpu.segments, Segments::flat32());
    let zero = CpuState::from_bytes([0; CpuState::BYTE_LEN]);
    assert_eq!(zero.to_bytes(), [0; CpuState::BYTE_LEN]);
    assert_eq!(zero.segments, Segments::default());
    for segment in Segment::ALL {
        assert!(!zero.segments[segment].attributes.is_usable());
        assert!(cpu.segments[segment].attributes.is_usable());
        assert_eq!(cpu.segments[segment].selector, 0);
    }
}

#[test]
fn flat_and_unusable_constructors_keep_the_visible_selector() {
    let data = StoredSegment::flat_data32(0x23);
    let code = StoredSegment::flat_code32(0x1b);
    let unusable = StoredSegment::unusable(0x37);
    assert_eq!(
        (data.base, data.limit, data.selector, data.attributes.bits()),
        (0, u32::MAX, 0x23, 0x15)
    );
    assert_eq!(
        (code.base, code.limit, code.selector, code.attributes.bits()),
        (0, u32::MAX, 0x1b, 0x17)
    );
    assert_eq!(
        (
            unusable.base,
            unusable.limit,
            unusable.selector,
            unusable.attributes.bits()
        ),
        (0, 0, 0x37, 0)
    );
}
