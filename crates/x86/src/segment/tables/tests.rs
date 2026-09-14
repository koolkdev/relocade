mod loads;

use crate::{
    DescriptorTables, Exception, Segment, SegmentAttributes, SegmentDefaultSize, SegmentDescriptor,
    SegmentDescriptorKind, StoredSegment,
};

fn data() -> SegmentDescriptor {
    SegmentDescriptor::new(
        0x1234_5000,
        0xffff,
        SegmentDescriptorKind::Data {
            writable: true,
            expand_down: false,
        },
        SegmentDefaultSize::Bits32,
    )
}

#[test]
fn selector_rpl_aliases_slots_while_table_and_index_remain_distinct() {
    let mut tables = DescriptorTables::default();
    let first = data();
    let second = SegmentDescriptor {
        base: 0x8888_0000,
        ..first
    };
    assert_eq!(tables.insert(0x23, first), None);
    assert_eq!(tables.insert(0x27, second), None);
    for selector in [0x20, 0x21, 0x22, 0x23] {
        assert_eq!(tables.get(selector), Some(&first));
    }
    for selector in [0x24, 0x25, 0x26, 0x27] {
        assert_eq!(tables.get(selector), Some(&second));
    }
    assert_eq!(tables.get(0x2b), None);
    assert_eq!(tables.insert(0x21, second), Some(first));
    assert_eq!(tables.remove(0x22), Some(second));
    assert_eq!(tables.get(0x23), None);
    assert_eq!(tables.get(0x27), Some(&second));
    assert_eq!(tables.remove(0x23), None);
    for (selector, alias) in [(0xfffb, 0xfff8), (0xffff, 0xfffc)] {
        assert_eq!(tables.insert(selector, first), None);
        assert_eq!(tables.get(alias), Some(&first));
    }
    assert_eq!(tables.get(0xffe3), None);
}

#[test]
fn only_global_slot_zero_is_a_null_selector() {
    let mut tables = DescriptorTables::default();
    tables.insert(0, data());
    tables.insert(4, data());
    for selector in 0..4 {
        for segment in [Segment::Ds, Segment::Es, Segment::Fs, Segment::Gs] {
            assert_eq!(
                tables.resolve_user_segment(segment, selector),
                Ok(StoredSegment::unusable(selector)),
            );
        }
        for segment in [Segment::Cs, Segment::Ss] {
            assert_eq!(
                tables.resolve_user_segment(segment, selector),
                Err(Exception::GeneralProtection { error_code: 0 }),
            );
        }
    }
    for selector in 4..8 {
        assert_eq!(
            tables.resolve_user_segment(Segment::Ds, selector),
            Ok(StoredSegment {
                base: 0x1234_5000,
                limit: 0xffff,
                selector,
                attributes: SegmentAttributes::from_bits(0x15),
            }),
        );
    }
}

#[test]
fn descriptor_replacement_and_removal_leave_loaded_copies_intact() {
    let mut tables = DescriptorTables::default();
    tables.insert(0x27, data());
    let loaded = tables.resolve_user_segment(Segment::Fs, 0x27).unwrap();
    let expected = StoredSegment {
        base: 0x1234_5000,
        limit: 0xffff,
        selector: 0x27,
        attributes: SegmentAttributes::from_bits(0x15),
    };
    tables.insert(
        0x24,
        SegmentDescriptor {
            base: 0x8765_0000,
            ..data()
        },
    );
    assert_eq!(loaded, expected);
    assert_eq!(
        tables.resolve_user_segment(Segment::Fs, 0x27).unwrap().base,
        0x8765_0000,
    );
    tables.remove(0x26);
    assert_eq!(loaded, expected);
    assert_eq!(
        tables.resolve_user_segment(Segment::Fs, 0x27),
        Err(Exception::GeneralProtection { error_code: 0x24 }),
    );
}
