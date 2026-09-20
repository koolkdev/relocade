use super::*;
use crate::{PrivilegeLevel, SegmentDescriptorInfo};

#[test]
fn every_user_code_and_data_type_is_visible_including_execute_only_code() {
    use SegmentDescriptorKind::{Code, Data};
    for (kind, access_rights) in [
        (
            Data {
                writable: false,
                expand_down: false,
            },
            0xf100,
        ),
        (
            Data {
                writable: true,
                expand_down: false,
            },
            0xf300,
        ),
        (
            Data {
                writable: false,
                expand_down: true,
            },
            0xf500,
        ),
        (
            Data {
                writable: true,
                expand_down: true,
            },
            0xf700,
        ),
        (
            Code {
                readable: false,
                conforming: false,
            },
            0xf900,
        ),
        (
            Code {
                readable: true,
                conforming: false,
            },
            0xfb00,
        ),
        (
            Code {
                readable: false,
                conforming: true,
            },
            0xfd00,
        ),
        (
            Code {
                readable: true,
                conforming: true,
            },
            0xff00,
        ),
    ] {
        let descriptor = SegmentDescriptor {
            kind,
            default_size: SegmentDefaultSize::Bits16,
            ..data()
        };
        let mut tables = DescriptorTables::default();
        tables.insert(0x27, descriptor);
        for selector in 0x24..=0x27 {
            let descriptor_info = tables.query_user_segment_descriptor(selector);
            assert!(descriptor_info.visible);
            assert_eq!(descriptor_info.access_rights, access_rights);
            assert_eq!(descriptor_info.limit, 0xffff);
        }
        assert_eq!(tables.get(0x27), Some(&descriptor));
    }
}

#[test]
fn visibility_uses_privilege_and_conformance_without_requiring_presence() {
    use PrivilegeLevel::*;
    for (dpl, rights) in [
        (Ring0, 0x1d00),
        (Ring1, 0x3d00),
        (Ring2, 0x5d00),
        (Ring3, 0x7d00),
    ] {
        for conforming in [false, true] {
            let mut tables = DescriptorTables::default();
            tables.insert(
                0x27,
                SegmentDescriptor {
                    kind: SegmentDescriptorKind::Code {
                        readable: false,
                        conforming,
                    },
                    dpl,
                    present: false,
                    default_size: SegmentDefaultSize::Bits16,
                    ..data()
                },
            );
            for selector in 0x24..=0x27 {
                let descriptor_info = tables.query_user_segment_descriptor(selector);
                if conforming || dpl == Ring3 {
                    assert!(descriptor_info.visible);
                    assert!(!descriptor_info.readable);
                    assert!(!descriptor_info.writable);
                    assert_eq!(
                        descriptor_info.access_rights,
                        if conforming { rights } else { 0x7900 }
                    );
                } else {
                    assert_eq!(descriptor_info, SegmentDescriptorInfo::default());
                }
            }
        }
    }
}

#[test]
fn query_retains_descriptor_flags_and_expands_the_same_limit_as_a_load() {
    use SegmentDefaultSize::*;
    for (present, size, available, limit, rights, effective) in [
        (
            false,
            Bits16,
            false,
            SegmentLimit::bytes(0).unwrap(),
            0x0000_7300,
            0,
        ),
        (
            true,
            Bits16,
            false,
            SegmentLimit::bytes(0xffff).unwrap(),
            0x0000_f300,
            0xffff,
        ),
        (
            true,
            Bits32,
            false,
            SegmentLimit::bytes(0xffff).unwrap(),
            0x0040_f300,
            0xffff,
        ),
        (
            true,
            Bits16,
            true,
            SegmentLimit::bytes(0xfffff).unwrap(),
            0x0010_f300,
            0xfffff,
        ),
        (
            true,
            Bits16,
            false,
            SegmentLimit::pages(0xf).unwrap(),
            0x0080_f300,
            0xffff,
        ),
        (
            true,
            Bits32,
            true,
            SegmentLimit::pages(0x12345).unwrap(),
            0x00d0_f300,
            0x1234_5fff,
        ),
        (
            true,
            Bits16,
            false,
            SegmentLimit::pages(0xfffff).unwrap(),
            0x0080_f300,
            u32::MAX,
        ),
    ] {
        let descriptor = SegmentDescriptor {
            present,
            default_size: size,
            available,
            limit,
            ..data()
        };
        let mut tables = DescriptorTables::default();
        tables.insert(0x27, descriptor);
        assert_eq!(
            tables.query_user_segment_descriptor(0x27),
            SegmentDescriptorInfo {
                visible: true,
                readable: true,
                writable: true,
                access_rights: rights,
                limit: effective,
            }
        );
        if present {
            assert_eq!(
                tables
                    .resolve_user_segment(Segment::Ds, 0x27)
                    .unwrap()
                    .limit,
                effective
            );
        }
        assert_eq!(tables.get(0x27), Some(&descriptor));
    }
}

#[test]
fn null_and_missing_selectors_are_invisible_but_local_zero_and_last_slots_exist() {
    let mut tables = DescriptorTables::default();
    for slot in [0, 4, 0xfff8, 0xfffc] {
        tables.insert(slot, data());
    }
    for selector in [0, 1, 2, 3, 0x24, 0xfff0] {
        assert_eq!(
            tables.query_user_segment_descriptor(selector),
            SegmentDescriptorInfo::default()
        );
    }
    for slot in [4, 0xfff8, 0xfffc] {
        for rpl in 0..4 {
            assert!(tables.query_user_segment_descriptor(slot | rpl).visible);
        }
    }
    tables.remove(0xffff);
    assert_eq!(
        tables.query_user_segment_descriptor(0xfffc),
        SegmentDescriptorInfo::default()
    );
}
