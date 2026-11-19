use super::{MmioHandlerId, PhysicalMapError, PhysicalMapping, PhysicalMemoryMap};

use PhysicalMapping::{Ram, Rom, Unmapped};

#[test]
fn code_pages_mark_all_backing_aliases_without_changing_routing() {
    let mut map = PhysicalMemoryMap::new([
        (
            0..=0xfff,
            Ram {
                backing_offset: 0x4000,
            },
        ),
        (
            0x1000..=0x1fff,
            Rom {
                backing_offset: 0x4000,
            },
        ),
        (
            0x2000..=0x2fff,
            PhysicalMapping::Mmio {
                handler: MmioHandlerId::new(0x4000),
            },
        ),
        (
            0x3000..=0x3fff,
            Ram {
                backing_offset: 0x5000,
            },
        ),
        (
            0x100000..=0x100fff,
            Ram {
                backing_offset: 0x4000,
            },
        ),
    ])
    .unwrap();
    let original = map.to_bytes();
    map.set_code_page(0x4123, true);
    let mut expected = original.clone();
    for (page, kind) in [(0, 5u32), (1, 6), (0x100, 5)] {
        expected[page * 8..page * 8 + 4].copy_from_slice(&kind.to_le_bytes());
    }
    assert_eq!(map.as_bytes(), expected);
    assert_eq!(
        map.get(0x100123),
        Ram {
            backing_offset: 0x4123
        }
    );
    assert_eq!(
        map.get(0x1123),
        Rom {
            backing_offset: 0x4123
        }
    );
    assert!(map.fits_backing(0x6000));
    map.set_code_page(0x4fff, false);
    assert_eq!(map.as_bytes(), original);
}

#[test]
fn mmio_handler_indices_use_the_existing_payload_word() {
    let mapping = PhysicalMapping::Mmio {
        handler: MmioHandlerId::new(7),
    };
    let map = PhysicalMemoryMap::new([
        (0x1000..=0x1fff, PhysicalMapping::MMIO),
        (0x2000..=0x3fff, mapping),
        (0xffff_f000..=u32::MAX, PhysicalMapping::MMIO),
    ])
    .unwrap();
    assert_eq!(map.get(0x2123), mapping);
    assert_eq!(map.get(0x3fff), mapping);
    assert!(map.fits_backing(0));
    assert_eq!(
        map.mmio_handler_offsets().collect::<Vec<_>>(),
        [
            (0x1000, 12),
            (0x2000, 20),
            (0x3000, 28),
            (0xffff_f000, 0x7f_fffc)
        ]
    );
    let bytes = map.as_bytes();
    assert_eq!(&bytes[8..16], &[3, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(&bytes[16..24], &[3, 0, 0, 0, 7, 0, 0, 0]);
    assert_eq!(&bytes[24..32], &[3, 0, 0, 0, 7, 0, 0, 0]);
}

#[test]
fn empty_regions_produce_a_complete_unmapped_table() {
    let map = PhysicalMemoryMap::new([]).unwrap();
    for address in [0, 0xa0000, 0x10ffef, 0x110000, u32::MAX] {
        assert_eq!(map.get(address), Unmapped);
    }
    let bytes = map.as_bytes();
    assert!(bytes[..PhysicalMemoryMap::A20_MASK_OFFSET]
        .iter()
        .all(|&byte| byte == 0));
    assert_eq!(
        &bytes[PhysicalMemoryMap::A20_MASK_OFFSET..],
        &[0xff, 0xff, 0xef, 0xff]
    );
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
        (0x2000..=0x3fff, PhysicalMapping::MMIO),
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
    assert_eq!(map.get(0x2fff), PhysicalMapping::MMIO);
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
    let mut map = PhysicalMemoryMap::new([
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
    map.set_a20_enabled(true);
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
    let mut map = PhysicalMemoryMap::new([
        (0..=0x10ffff, Ram { backing_offset: 0 }),
        (
            0x10f000..=0x10ffff,
            Rom {
                backing_offset: 0xffff_f000,
            },
        ),
    ])
    .unwrap();
    map.set_a20_enabled(true);
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
fn full_u32_ram_range_accepts_the_last_backing_byte() {
    let mut map = PhysicalMemoryMap::new([(0..=u32::MAX, Ram { backing_offset: 0 })]).unwrap();
    assert_eq!(
        map.get(u32::MAX),
        Ram {
            backing_offset: 0xffef_ffff
        }
    );
    map.set_a20_enabled(true);
    assert_eq!(
        map.get(u32::MAX),
        Ram {
            backing_offset: u32::MAX
        }
    );
    assert!(!map.fits_backing(u64::from(u32::MAX)));
    assert!(map.fits_backing(1u64 << 32));
}

#[test]
fn serialization_has_fixed_little_endian_entries_without_a_header() {
    let map = PhysicalMemoryMap::new([
        (0..=0xfff, PhysicalMapping::MMIO),
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
            0xffff_f000..=u32::MAX,
            Ram {
                backing_offset: 0xffff_f000,
            },
        ),
    ])
    .unwrap();
    let bytes = map.as_bytes();
    assert_eq!(bytes.len(), 0x80_0004);
    assert_eq!(
        &bytes[..24],
        &[3, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0x50, 0x34, 0x12, 2, 0, 0, 0, 0, 0x90, 0, 0,]
    );
    assert!(bytes[24..0x7f_fff8].iter().all(|&byte| byte == 0));
    assert_eq!(
        &bytes[0x7f_fff8..0x80_0000],
        &[1, 0, 0, 0, 0, 0xf0, 0xff, 0xff]
    );
    assert_eq!(
        &bytes[PhysicalMemoryMap::A20_MASK_OFFSET..],
        &[0xff, 0xff, 0xef, 0xff]
    );
}

#[test]
fn a20_defaults_to_disabled_and_toggling_preserves_mappings() {
    let mut map = PhysicalMemoryMap::new([
        (0x10000..=0x10fff, PhysicalMapping::MMIO),
        (
            0..=0xfff,
            Ram {
                backing_offset: 0x2000,
            },
        ),
        (
            0x100000..=0x100fff,
            Rom {
                backing_offset: 0x5000,
            },
        ),
    ])
    .unwrap();
    assert_eq!(
        map.get(0x100123),
        Ram {
            backing_offset: 0x2123
        }
    );
    assert_eq!(
        map.get(0x123),
        Ram {
            backing_offset: 0x2123
        }
    );
    assert_eq!(map.get(u32::MAX), Unmapped);
    assert_eq!(map.get(0x110123), PhysicalMapping::MMIO);
    let disabled = map.to_bytes();
    assert_eq!(
        &disabled[PhysicalMemoryMap::A20_MASK_OFFSET..],
        &[0xff, 0xff, 0xef, 0xff]
    );
    map.set_a20_enabled(true);
    assert_eq!(map.get(0x110123), Unmapped);
    assert_eq!(
        map.get(0x100123),
        Rom {
            backing_offset: 0x5123
        }
    );
    assert_eq!(
        &map.as_bytes()[..PhysicalMemoryMap::A20_MASK_OFFSET],
        &disabled[..PhysicalMemoryMap::A20_MASK_OFFSET]
    );
    assert_eq!(
        &map.as_bytes()[PhysicalMemoryMap::A20_MASK_OFFSET..],
        &[0xff; 4]
    );
    map.set_a20_enabled(false);
    assert_eq!(map.as_bytes(), disabled);
}
