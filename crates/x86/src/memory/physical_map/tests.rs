use super::{PhysicalMapError, PhysicalMapping, PhysicalMemoryMap};

use PhysicalMapping::{Mmio, Ram, Rom, Unmapped};

#[test]
fn empty_regions_produce_a_complete_unmapped_table() {
    let map = PhysicalMemoryMap::new([]).unwrap();
    for address in [0, 0xa0000, 0x10ffef, 0x110000, u32::MAX] {
        assert_eq!(map.get(address), Unmapped);
    }
    assert_eq!(map.to_bytes(), [0; 2176]);
}

#[test]
fn direct_ranges_translate_bytes_and_preserve_ram_and_rom_kinds() {
    let map = PhysicalMemoryMap::new([
        (
            0x2000..=0x4fff,
            Ram {
                backing_offset: 0x8000,
            },
        ),
        (
            0xf0000..=0xfffff,
            Rom {
                backing_offset: 0x10000,
            },
        ),
    ])
    .unwrap();
    for (address, expected) in [
        (0x1fff, Unmapped),
        (
            0x2000,
            Ram {
                backing_offset: 0x8000,
            },
        ),
        (
            0x3123,
            Ram {
                backing_offset: 0x9123,
            },
        ),
        (
            0x4fff,
            Ram {
                backing_offset: 0xafff,
            },
        ),
        (0x5000, Unmapped),
        (
            0xfffff,
            Rom {
                backing_offset: 0x1ffff,
            },
        ),
        (0x100000, Unmapped),
    ] {
        assert_eq!(map.get(address), expected, "physical address {address:#x}");
    }
}

#[test]
fn later_regions_replace_only_the_requested_pages() {
    let map = PhysicalMemoryMap::new([
        (
            0..=0x4fff,
            Ram {
                backing_offset: 0x9000,
            },
        ),
        (
            0x1000..=0x1fff,
            Rom {
                backing_offset: 0x5000,
            },
        ),
        (0x2000..=0x3fff, Mmio),
        (0x3000..=0x3fff, Unmapped),
    ])
    .unwrap();
    assert_eq!(
        map.get(0xfff),
        Ram {
            backing_offset: 0x9fff
        }
    );
    assert_eq!(
        map.get(0x1000),
        Rom {
            backing_offset: 0x5000
        }
    );
    assert_eq!(map.get(0x2fff), Mmio);
    assert_eq!(map.get(0x3000), Unmapped);
    assert_eq!(
        map.get(0x4000),
        Ram {
            backing_offset: 0xd000
        }
    );
}

#[test]
fn separate_physical_ranges_can_alias_the_same_backing() {
    let map = PhysicalMemoryMap::new([
        (
            0..=0x1fff,
            Ram {
                backing_offset: 0x4000,
            },
        ),
        (
            0x100000..=0x101fff,
            Ram {
                backing_offset: 0x4000,
            },
        ),
    ])
    .unwrap();
    assert_eq!(
        map.get(0x1234),
        Ram {
            backing_offset: 0x5234
        }
    );
    assert_eq!(
        map.get(0x101234),
        Ram {
            backing_offset: 0x5234
        }
    );
}

#[test]
fn constructor_rejects_invalid_regions() {
    for (range, mapping, expected) in [
        (
            std::ops::RangeInclusive::new(0x2000, 0xfff),
            Unmapped,
            PhysicalMapError::EmptyRange,
        ),
        (1..=0x1fff, Unmapped, PhysicalMapError::UnalignedRange),
        (0..=0x1ffe, Unmapped, PhysicalMapError::UnalignedRange),
        (
            0x110000..=0x110fff,
            Mmio,
            PhysicalMapError::RangeOutOfBounds,
        ),
        (0..=0x110fff, Unmapped, PhysicalMapError::RangeOutOfBounds),
        (0..=u32::MAX, Unmapped, PhysicalMapError::RangeOutOfBounds),
        (
            0..=0xfff,
            Ram { backing_offset: 1 },
            PhysicalMapError::UnalignedBacking,
        ),
        (
            0..=0x1fff,
            Rom {
                backing_offset: 0xffff_f000,
            },
            PhysicalMapError::BackingOverflow,
        ),
    ] {
        assert_eq!(
            PhysicalMemoryMap::new([(range, mapping)]).unwrap_err(),
            expected
        );
    }
}

#[test]
fn last_real_mode_page_can_use_the_last_32_bit_backing_page() {
    let map = PhysicalMemoryMap::new([
        (0..=0x10ffff, Ram { backing_offset: 0 }),
        (
            0x10f000..=0x10ffff,
            Rom {
                backing_offset: 0xffff_f000,
            },
        ),
    ])
    .unwrap();
    assert_eq!(
        map.get(0x10efff),
        Ram {
            backing_offset: 0x10efff
        }
    );
    assert_eq!(
        map.get(0x10ffef),
        Rom {
            backing_offset: 0xffff_ffef
        }
    );
    assert_eq!(
        map.get(0x10ffff),
        Rom {
            backing_offset: u32::MAX
        }
    );
    assert_eq!(map.get(0x110000), Unmapped);
}

#[test]
fn serialization_has_fixed_little_endian_entries_without_a_header() {
    let map = PhysicalMemoryMap::new([
        (0..=0xfff, Mmio),
        (
            0x1000..=0x1fff,
            Ram {
                backing_offset: 0x1234_5000,
            },
        ),
        (
            0x2000..=0x2fff,
            Rom {
                backing_offset: 0x9000,
            },
        ),
        (
            0x10f000..=0x10ffff,
            Ram {
                backing_offset: 0xffff_f000,
            },
        ),
    ])
    .unwrap();
    let bytes = map.to_bytes();
    assert_eq!(bytes.len(), 2176);
    assert_eq!(
        &bytes[..24],
        &[3, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0x50, 0x34, 0x12, 2, 0, 0, 0, 0, 0x90, 0, 0,]
    );
    assert!(bytes[24..2168].iter().all(|&byte| byte == 0));
    assert_eq!(&bytes[2168..], &[1, 0, 0, 0, 0, 0xf0, 0xff, 0xff]);
}
