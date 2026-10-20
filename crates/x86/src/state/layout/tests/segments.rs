use std::mem::{offset_of, size_of};

use crate::{CpuState, Segment, SegmentRegister, Segments};

#[test]
fn segment_records_match_the_external_byte_contract() {
    assert_eq!(size_of::<SegmentRegister>(), 16);
    assert_eq!(size_of::<Segments>(), 96);
    assert_eq!(
        [
            offset_of!(SegmentRegister, base),
            offset_of!(SegmentRegister, limit),
            offset_of!(SegmentRegister, selector),
            offset_of!(SegmentRegister, usable),
            offset_of!(SegmentRegister, readable),
            offset_of!(SegmentRegister, writable),
            offset_of!(SegmentRegister, executable),
            offset_of!(SegmentRegister, expand_down),
            offset_of!(SegmentRegister, big),
        ],
        [0, 4, 8, 10, 11, 12, 13, 14, 15]
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
        [64, 80, 96, 112, 128, 144]
    );
}

#[test]
fn decoding_retains_every_segment_field_including_noncanonical_boolean_bytes() {
    let bytes = std::array::from_fn(|index| index as u8);
    let cpu = CpuState::from_bytes(bytes);
    for (segment, start) in Segment::ALL.into_iter().zip([64u8, 80, 96, 112, 128, 144]) {
        let value = |offset| start + offset;
        assert_eq!(
            cpu.segments[segment],
            SegmentRegister {
                base: u32::from_le_bytes([value(0), value(1), value(2), value(3)]),
                limit: u32::from_le_bytes([value(4), value(5), value(6), value(7)]),
                selector: u16::from_le_bytes([value(8), value(9)]),
                usable: value(10),
                readable: value(11),
                writable: value(12),
                executable: value(13),
                expand_down: value(14),
                big: value(15),
            },
            "{segment:?}"
        );
    }
    assert_eq!(cpu.to_bytes(), bytes);
}

#[test]
fn encoding_a_named_segment_changes_only_its_sixteen_bytes() {
    for (segment, offset) in Segment::ALL.into_iter().zip([64, 80, 96, 112, 128, 144]) {
        let mut cpu = CpuState::filled(0xa5);
        cpu.segments[segment] = SegmentRegister {
            base: 0x1234_5678,
            limit: 0xfedc_ba98,
            selector: 0x1357,
            usable: 0x80,
            readable: 1,
            writable: 0,
            executable: 0xff,
            expand_down: 0x82,
            big: 2,
        };
        let mut expected = [0xa5; CpuState::BYTE_LEN];
        expected[offset..offset + 16].copy_from_slice(&[
            0x78, 0x56, 0x34, 0x12, 0x98, 0xba, 0xdc, 0xfe, 0x57, 0x13, 0x80, 1, 0, 0xff, 0x82, 2,
        ]);
        assert_eq!(cpu.to_bytes(), expected, "{segment:?}");
        assert_eq!(CpuState::from_bytes(expected), cpu);
    }
}

#[test]
fn cpu_default_installs_flat_caches_while_literal_zero_images_stay_zero() {
    let cpu = CpuState::default();
    let mut expected = [0; 176];
    for (offset, writable, executable) in [
        (64, 1, 0),
        (80, 0, 1),
        (96, 1, 0),
        (112, 1, 0),
        (128, 1, 0),
        (144, 1, 0),
    ] {
        expected[offset..offset + 16].copy_from_slice(&[
            0, 0, 0, 0, 0xff, 0xff, 0xff, 0xff, 0, 0, 1, 1, writable, executable, 0, 1,
        ]);
    }
    assert_eq!(&cpu.to_bytes()[..176], &expected);
    assert_eq!(cpu.segments, Segments::flat32());
    let zero = CpuState::from_bytes([0; CpuState::BYTE_LEN]);
    assert_eq!(zero.to_bytes(), [0; CpuState::BYTE_LEN]);
    assert_eq!(zero.segments, Segments::default());
}

#[test]
fn flat_and_unusable_constructors_keep_the_visible_selector() {
    let data = SegmentRegister::flat_data32(0x23);
    let code = SegmentRegister::flat_code32(0x1b);
    assert_eq!(
        (
            data.base,
            data.limit,
            data.selector,
            data.usable,
            data.readable,
            data.writable,
            data.executable,
            data.expand_down,
            data.big
        ),
        (0, u32::MAX, 0x23, 1, 1, 1, 0, 0, 1)
    );
    assert_eq!(
        code,
        SegmentRegister {
            selector: 0x1b,
            writable: 0,
            executable: 1,
            ..data
        }
    );
    assert_eq!(
        SegmentRegister::unusable(0x37),
        SegmentRegister {
            selector: 0x37,
            ..SegmentRegister::default()
        }
    );
}
