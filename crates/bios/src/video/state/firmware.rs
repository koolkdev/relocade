//! The IBM BIOS-data block, including guest-replaceable interrupt vectors.
use relocade_machine::MemoryView;

pub(super) const SIZE: usize = 0x3a;

// Physical address, buffer offset, byte count. Equipment bits are merged separately.
const FIELDS: [(u32, usize, usize); 7] = [
    (0x449, 0x01, 30),
    (0x484, 0x1f, 7),
    (0x4a8, 0x26, 4),
    (0x014, 0x2a, 4), // INT 05h.
    (0x074, 0x2e, 4), // INT 1Dh.
    (0x07c, 0x32, 4), // INT 1Fh.
    (0x10c, 0x36, 4), // INT 43h.
];

pub(super) fn save(memory: &MemoryView<'_>) -> [u8; SIZE] {
    let mut bytes = [0; SIZE];
    bytes[0] = memory.read_u8(0x410).expect("equipment byte") & 0x30;
    for (address, offset, count) in FIELDS {
        memory
            .read(address, &mut bytes[offset..offset + count])
            .expect("BIOS video data");
    }
    bytes
}

pub(super) fn restore(memory: &mut MemoryView<'_>, bytes: &[u8; SIZE]) {
    let equipment = memory.read_u8(0x410).expect("equipment byte");
    memory
        .write_u8(0x410, (equipment & !0x30) | (bytes[0] & 0x30))
        .expect("equipment byte");
    for (address, offset, count) in FIELDS {
        memory
            .write(address, &bytes[offset..offset + count])
            .expect("BIOS video data");
    }
}
