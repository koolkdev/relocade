use super::{SegmentAttributes, SegmentDefaultSize, SegmentKind};

#[test]
fn usable_kinds_have_explicit_normalized_attribute_encodings() {
    for (kind, bits16, bits32) in [
        (
            SegmentKind::Data {
                writable: false,
                expand_down: false,
            },
            0x01,
            0x11,
        ),
        (
            SegmentKind::Data {
                writable: true,
                expand_down: false,
            },
            0x05,
            0x15,
        ),
        (
            SegmentKind::Data {
                writable: false,
                expand_down: true,
            },
            0x09,
            0x19,
        ),
        (
            SegmentKind::Data {
                writable: true,
                expand_down: true,
            },
            0x0d,
            0x1d,
        ),
        (SegmentKind::Code { readable: false }, 0x03, 0x13),
        (SegmentKind::Code { readable: true }, 0x07, 0x17),
    ] {
        for (size, bits) in [
            (SegmentDefaultSize::Bits16, bits16),
            (SegmentDefaultSize::Bits32, bits32),
        ] {
            let attributes = SegmentAttributes::new(kind, size);
            assert_eq!(attributes.bits(), bits);
            assert!(attributes.is_usable());
            let stored = SegmentAttributes::from_bits(bits);
            assert_eq!(stored.kind(), Some(kind));
            assert_eq!(stored.default_size(), size);
        }
    }
}

#[test]
fn raw_attributes_preserve_reserved_bits_without_changing_the_loaded_type() {
    let attributes = SegmentAttributes::from_bits(0xa5f5);
    assert_eq!(attributes.bits(), 0xa5f5);
    assert_eq!(
        attributes.kind(),
        Some(SegmentKind::Data {
            writable: true,
            expand_down: false
        })
    );
    assert_eq!(attributes.default_size(), SegmentDefaultSize::Bits32);
    assert_eq!(SegmentAttributes::from_bits(0xffff).bits(), 0xffff);
}

#[test]
fn unusable_or_invalid_attribute_combinations_have_no_usable_kind() {
    assert_eq!(SegmentAttributes::default(), SegmentAttributes::unusable());
    for bits in [0, 0x1e, 0xfffe] {
        let attributes = SegmentAttributes::from_bits(bits);
        assert!(!attributes.is_usable());
        assert_eq!(attributes.kind(), None);
        assert_eq!(attributes.bits(), bits);
    }
    for bits in [0x0b, 0x0f, 0x1b, 0x1f, 0xffff] {
        let attributes = SegmentAttributes::from_bits(bits);
        assert!(attributes.is_usable());
        assert_eq!(
            attributes.kind(),
            None,
            "code cannot expand down: {bits:04x}"
        );
        assert_eq!(attributes.bits(), bits);
    }
}
