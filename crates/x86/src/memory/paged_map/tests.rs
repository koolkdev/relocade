use super::*;

#[test]
fn sparse_pages_encode_permissions_aliases_and_the_last_address() {
    let mut map = PagedMemoryMap::default();
    map.map(0x1000..=0x2fff, 0x5000, true).unwrap();
    map.map(0x80000000..=0x80000fff, 0x5000, false).unwrap();
    map.map(0xfffff000..=u32::MAX, 0xfffff000, true).unwrap();
    assert_eq!(map.required_backing_bytes(), 0x1_0000_0000);
    let bytes = map.to_bytes();
    assert_eq!(&bytes[4..12], &[3, 0x50, 0, 0, 3, 0x60, 0, 0]);
    assert_eq!(&bytes[0x200000..0x200004], &[1, 0x50, 0, 0]);
    assert_eq!(&bytes[0x3ffffc..], &[3, 0xf0, 0xff, 0xff]);
    assert_eq!(&bytes[0..4], &[0; 4]);
    map.unmap(0xfffff000..=u32::MAX).unwrap();
    assert_eq!(map.required_backing_bytes(), 0x7000);
    assert_eq!(map.entry(u32::MAX), 0);
    assert_eq!(map.entry(0x80000fff), 0x5001);
}

#[test]
fn invalid_replacements_leave_existing_pages_unchanged() {
    let mut map = PagedMemoryMap::default();
    map.map(0x1000..=0x1fff, 0, true).unwrap();
    assert_eq!(
        map.map(0x1001..=0x1fff, 0, false),
        Err(PagedMapError::UnalignedRange)
    );
    assert_eq!(
        map.map(0x1000..=0x2000, 0, false),
        Err(PagedMapError::UnalignedRange)
    );
    assert_eq!(
        map.map(0x1000..=0x1fff, 1, false),
        Err(PagedMapError::UnalignedBacking)
    );
    assert_eq!(
        map.map(0x1000..=0x2fff, 0xfffff000, false),
        Err(PagedMapError::BackingOverflow)
    );
    assert_eq!(
        map.map(std::ops::RangeInclusive::new(0x2000, 0x1fff), 0, false),
        Err(PagedMapError::EmptyRange)
    );
    assert_eq!(map.entry(0x1000), 3);
    assert_eq!(map.required_backing_bytes(), 4096);
}
