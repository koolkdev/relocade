use super::*;
use crate::PrivilegeLevel;

fn permissions(tables: &DescriptorTables, selector: u16) -> (bool, bool) {
    let descriptor_info = tables.query_user_segment_descriptor(selector);
    (descriptor_info.readable, descriptor_info.writable)
}

#[test]
fn permissions_follow_type_and_privilege_independently_of_presence_or_extent() {
    use SegmentDescriptorKind::{Code, Data};
    let dpls = [
        PrivilegeLevel::Ring0,
        PrivilegeLevel::Ring1,
        PrivilegeLevel::Ring2,
        PrivilegeLevel::Ring3,
    ];
    // Each row gives the read/write mask at DPL 0, 1, 2 and 3, at fixed CPL 3.
    for (kind, expected) in [
        (
            Data {
                writable: false,
                expand_down: false,
            },
            [0, 0, 0, 1],
        ),
        (
            Data {
                writable: true,
                expand_down: false,
            },
            [0, 0, 0, 3],
        ),
        (
            Data {
                writable: false,
                expand_down: true,
            },
            [0, 0, 0, 1],
        ),
        (
            Data {
                writable: true,
                expand_down: true,
            },
            [0, 0, 0, 3],
        ),
        (
            Code {
                readable: false,
                conforming: false,
            },
            [0, 0, 0, 0],
        ),
        (
            Code {
                readable: true,
                conforming: false,
            },
            [0, 0, 0, 1],
        ),
        (
            Code {
                readable: false,
                conforming: true,
            },
            [0, 0, 0, 0],
        ),
        (
            Code {
                readable: true,
                conforming: true,
            },
            [1, 1, 1, 1],
        ),
    ] {
        for (dpl, mask) in dpls.into_iter().zip(expected) {
            for present in [false, true] {
                for (limit, default_size) in [
                    (0, SegmentDefaultSize::Bits16),
                    (u32::MAX, SegmentDefaultSize::Bits32),
                ] {
                    let descriptor = SegmentDescriptor {
                        kind,
                        dpl,
                        present,
                        limit: SegmentLimit::from_effective(limit).unwrap(),
                        default_size,
                        ..data()
                    };
                    let mut tables = DescriptorTables::default();
                    tables.insert(0x27, descriptor);
                    for selector in 0x24..=0x27 {
                        assert_eq!(
                            permissions(&tables, selector),
                            (mask & 1 != 0, mask & 2 != 0),
                            "{descriptor:?}, selector {selector:#x}"
                        );
                    }
                    assert_eq!(tables.get(0x27), Some(&descriptor));
                }
            }
        }
    }
}

#[test]
fn null_missing_and_removed_slots_deny_both_permissions_but_local_zero_is_valid() {
    let mut tables = DescriptorTables::default();
    let none = (false, false);
    let both = (true, true);
    for selector in [0, 4, 0x20, 0xfff8, 0xfffc] {
        tables.insert(selector, data());
    }
    for selector in 0..4 {
        assert_eq!(permissions(&tables, selector), none);
    }
    for slot in [4, 0x20, 0xfff8, 0xfffc] {
        for rpl in 0..4 {
            assert_eq!(permissions(&tables, slot | rpl), both);
        }
    }
    for selector in [0x24, 0x28, 0xfff0, 0xfff4] {
        assert_eq!(permissions(&tables, selector), none);
    }
    tables.remove(0x23);
    assert_eq!(permissions(&tables, 0x20), none);
}

#[test]
fn a_nonpresent_descriptor_can_pass_verification_and_still_fail_a_load() {
    let mut tables = DescriptorTables::default();
    tables.insert(
        0x27,
        SegmentDescriptor {
            present: false,
            ..data()
        },
    );
    assert_eq!(permissions(&tables, 0x27), (true, true));
    assert_eq!(
        tables.resolve_user_segment(Segment::Ds, 0x27),
        Err(Exception::SegmentNotPresent { error_code: 0x24 })
    );
}
