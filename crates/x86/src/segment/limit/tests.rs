use super::SegmentLimit;

#[test]
fn encoded_limits_are_bounded_and_expand_with_their_granularity() {
    for (encoded, bytes, pages) in [
        (0, 0, 0xfff),
        (1, 1, 0x1fff),
        (0xffff, 0xffff, 0x0fff_ffff),
        (0xfffff, 0xfffff, u32::MAX),
    ] {
        let byte_limit = SegmentLimit::bytes(encoded).unwrap();
        let page_limit = SegmentLimit::pages(encoded).unwrap();
        assert_eq!(byte_limit.effective(), bytes);
        assert_eq!(page_limit.effective(), pages);
        assert!(!byte_limit.is_page_granular());
        assert!(page_limit.is_page_granular());
    }
    for encoded in [0x10_0000, u32::MAX] {
        assert_eq!(SegmentLimit::bytes(encoded), None);
        assert_eq!(SegmentLimit::pages(encoded), None);
    }
}

#[test]
fn effective_limits_convert_exactly_or_fail_without_rounding() {
    for limit in [0, 0xfff, 0xfffff, 0x10_0fff, 0x1234_5fff, u32::MAX] {
        let converted = SegmentLimit::from_effective(limit).unwrap();
        assert_eq!(converted.effective(), limit);
        assert_eq!(converted.is_page_granular(), limit > 0xfffff);
    }
    for limit in [0x10_0000, 0x10_0ffe, 0x1234_5678, 0xffff_fffe] {
        assert_eq!(SegmentLimit::from_effective(limit), None);
    }
}

#[test]
fn equal_byte_limits_can_retain_different_granularity_bits() {
    let byte_limit = SegmentLimit::bytes(0xffff).unwrap();
    let page_limit = SegmentLimit::pages(0xf).unwrap();
    assert_eq!(byte_limit.effective(), page_limit.effective());
    assert_ne!(byte_limit, page_limit);
}
