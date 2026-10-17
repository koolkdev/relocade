use super::{encode_entry, entry_offset, BYTE_LEN, PAGE_BYTES, UNMAPPED_ENTRY};

#[test]
fn entry_offsets_cover_the_first_and_last_linear_pages() {
    assert_eq!(PAGE_BYTES, 4096);
    assert_eq!(BYTE_LEN, 4 * 1024 * 1024);
    assert_eq!(entry_offset(0), 0);
    assert_eq!(entry_offset(0xfff), 0);
    assert_eq!(entry_offset(0x1000), 4);
    assert_eq!(entry_offset(0x8000_0123), 0x20_0000);
    assert_eq!(entry_offset(0xffff_f000), 0x3f_fffc);
    assert_eq!(entry_offset(u32::MAX), 0x3f_fffc);
}

#[test]
fn entries_encode_backing_and_permissions_in_little_endian_order() {
    assert_eq!(UNMAPPED_ENTRY, [0, 0, 0, 0]);
    assert_eq!(encode_entry(0, false), Some([1, 0, 0, 0]));
    assert_eq!(
        encode_entry(0x1234_5000, false),
        Some([1, 0x50, 0x34, 0x12])
    );
    assert_eq!(encode_entry(0x1234_5000, true), Some([3, 0x50, 0x34, 0x12]));
    assert_eq!(encode_entry(0xffff_f000, true), Some([3, 0xf0, 0xff, 0xff]));
    assert_eq!(encode_entry(1, false), None);
    assert_eq!(encode_entry(0x1234_5004, true), None);
    assert_eq!(encode_entry(u32::MAX, true), None);
}
