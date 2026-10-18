use crate::{FlagBytes, StoredFlags, StoredStatusSource};

#[test]
fn host_reads_resolve_each_status_source_width_without_materializing() {
    // Literal EFLAGS images include fixed bit 1. Dirty upper operands must not
    // affect byte/word recipes; logic ignores its right operand.
    for (kind, left, right, expected) in [
        (1, 0xabcd_1200, 0x1234_5601, 0x97), // byte 0 - 1
        (2, 0xabcd_12ff, 0x1234_5601, 0x57), // byte ff + 1
        (3, 0xffff_ff80, u32::MAX, 0x82),
        (5, 0xabcd_8000, 0x1234_0001, 0x816), // word 8000 - 1
        (6, 0xabcd_7fff, 0x1234_0001, 0x896), // word 7fff + 1
        (7, 0xffff_0000, u32::MAX, 0x46),
        (9, 0x8000_0000, 1, 0x816),
        (10, 0xffff_ffff, 1, 0x57),
        (11, 0x8000_0000, u32::MAX, 0x86),
    ] {
        let flags = StoredFlags {
            status_source: StoredStatusSource {
                kind,
                left,
                right,
                reserved: [0xa5; 3],
            },
            bytes: FlagBytes::default(),
        };
        let before = flags.to_bytes();
        assert_eq!(flags.eflags(), expected, "source kind {kind}");
        assert_eq!(flags.to_bytes(), before);
    }
}

#[test]
fn host_flag_images_use_low_bits_and_ignore_unrepresented_flags() {
    let mut flags = StoredFlags::from_bytes([0xff; StoredFlags::BYTE_LEN]);
    flags.status_source.kind = 0;
    flags.bytes.cf = 0x80;
    flags.bytes.if_ = 0xfe;
    assert_eq!(flags.eflags(), 0x247dd6);
    flags.set_eflags(u32::MAX);
    assert_eq!(flags.eflags(), 0x247fd7);
    assert_eq!(flags.status_source.kind, 0);
    assert_eq!(flags.status_source.left, u32::MAX);
    assert_eq!(flags.status_source.reserved, [0xff; 3]);
    assert_eq!(flags.bytes.reserved, [0xff; 3]);
    flags.set_eflags(0);
    assert_eq!(flags.eflags(), 2);
}
