use super::{MemoryError, MemoryView};
use crate::{MachineConfig, MemoryAccess};
use relocade_x86::{PhysicalMapping, PhysicalMemoryMap, Segment, SegmentRegister};

fn configuration(map: PhysicalMemoryMap) -> (MachineConfig, Vec<u8>) {
    (MachineConfig::new(0x3000, map).unwrap(), vec![0; 0x3000])
}

#[test]
fn transfers_cross_pages_with_their_guest_access_policy() {
    let map = PhysicalMemoryMap::new([
        (
            0..=0xfff,
            PhysicalMapping::Ram {
                backing_offset: 0x1000,
            },
        ),
        (0x1000..=0x1fff, PhysicalMapping::Rom { backing_offset: 0 }),
        (0x3000..=0x3fff, PhysicalMapping::Mmio),
    ])
    .unwrap();
    let (config, mut storage) = configuration(map);
    let mut memory = MemoryView::new(&config, MemoryAccess::from_bytes(&mut storage));
    memory.write_backing(0, &[0xaa, 0xbb]).unwrap();
    memory.write(0xffe, &[1, 2, 3, 4]).unwrap();
    let mut bytes = [0; 4];
    memory.read(0xffe, &mut bytes).unwrap();
    assert_eq!(bytes, [1, 2, 0xaa, 0xbb]);
    // Scalar accesses use little-endian order across the same RAM/ROM boundary.
    assert_eq!(memory.read_u16(0xfff).unwrap(), 0xaa02);
    memory.write_u16(0xfff, 0x7856).unwrap();
    assert_eq!(memory.read_u8(0xfff).unwrap(), 0x56);
    assert_eq!(memory.read_u16(0xfff).unwrap(), 0xaa56);
    memory.write_u8(0x1000, 0x99).unwrap();
    assert_eq!(memory.read_u8(0x1000).unwrap(), 0xaa);
    memory.write_backing(0xffe, &[0xcc, 0xdd]).unwrap();
    memory.read(0x1ffe, &mut bytes).unwrap();
    assert_eq!(bytes, [0xcc, 0xdd, 0xff, 0xff]);
    memory.write(0x2ffe, &[1, 2, 3, 4]).unwrap();
    memory.read(0x2ffe, &mut bytes).unwrap();
    assert_eq!(bytes, [0xff; 4]);
    memory.read(0x200000, &mut bytes).unwrap();
    assert_eq!(bytes, [0xff; 4]);

    memory.write_u64(0x21, 0xfedc_ba98_7654_3210).unwrap();
    let mut wide = [0; 8];
    memory.read_backing(0x1021, &mut wide).unwrap();
    assert_eq!(wide, [0x10, 0x32, 0x54, 0x76, 0x98, 0xba, 0xdc, 0xfe]);
    assert_eq!(memory.read_u32(0x22).unwrap(), 0x9876_5432);
    memory.write_u32(0x23, 0xdead_beef).unwrap();
    assert_eq!(memory.read_u64(0x21).unwrap(), 0xfedc_dead_beef_3210);

    memory
        .write_backing(0, &[0x88, 0x77, 0x66, 0x55, 0x44, 0x33, 0x22, 0x11])
        .unwrap();
    memory.write_u64(0x1000, 0).unwrap();
    assert_eq!(memory.read_u64(0x1000).unwrap(), 0x1122_3344_5566_7788);
    memory.write_u64(0xffc, 0xfedc_ba98_7654_3210).unwrap();
    assert_eq!(memory.read_u64(0xffc).unwrap(), 0x5566_7788_7654_3210);
    assert_eq!(memory.read_u32(0x1ffe).unwrap(), 0xffff_ddcc);
    memory.write_u32(0x3001, 0).unwrap();
    assert_eq!(memory.read_u32(0x3001).unwrap(), u32::MAX);
    assert_eq!(memory.read_u64(0x2001).unwrap(), u64::MAX);
}

#[test]
fn a20_aliases_and_enabled_high_memory_use_the_same_backing() {
    for enabled in [false, true] {
        let mut map = PhysicalMemoryMap::new([
            (0..=0xfff, PhysicalMapping::Ram { backing_offset: 0 }),
            (
                0x100000..=0x100fff,
                PhysicalMapping::Ram {
                    backing_offset: 0x2000,
                },
            ),
        ])
        .unwrap();
        map.set_a20_enabled(enabled);
        let (config, mut storage) = configuration(map);
        let mut memory = MemoryView::new(&config, MemoryAccess::from_bytes(&mut storage));
        memory.write_u32(0x100000, 0x1234_565a).unwrap();
        assert_eq!(memory.read_u32(0x100000).unwrap(), 0x1234_565a);
        let mut byte = [0];
        memory
            .read_backing(if enabled { 0x2000 } else { 0 }, &mut byte)
            .unwrap();
        assert_eq!(byte, [0x5a]);
        let ds = SegmentRegister::real_mode(Segment::Ds, 0xffff);
        memory.read_segment(&ds, 0x10, &mut byte).unwrap();
        assert_eq!(byte, [0x5a]);
        memory.write_segment(&ds, 0x10, &[0x42]).unwrap();
        memory.read(0x100000, &mut byte).unwrap();
        assert_eq!(byte, [0x42]);
    }
}

#[test]
fn segment_transfers_check_the_whole_span_before_copying() {
    let map = PhysicalMemoryMap::new([
        (0xf000..=0xffff, PhysicalMapping::Ram { backing_offset: 0 }),
        (
            0x10000..=0x10fff,
            PhysicalMapping::Ram {
                backing_offset: 0x1000,
            },
        ),
    ])
    .unwrap();
    let (config, mut storage) = configuration(map);
    let mut memory = MemoryView::new(&config, MemoryAccess::from_bytes(&mut storage));
    let ds = SegmentRegister::real_mode(Segment::Ds, 0);
    memory.write(0xfffe, b"ABCD").unwrap();
    for offset in [0xffff, 0x10000, u32::MAX] {
        let mut bytes = [0x55; 2];
        assert_eq!(
            memory.read_segment(&ds, offset, &mut bytes),
            Err(MemoryError::SegmentRange)
        );
        assert_eq!(bytes, [0x55; 2]);
        assert_eq!(
            memory.write_segment(&ds, offset, b"XY"),
            Err(MemoryError::SegmentRange)
        );
    }
    let mut bytes = [0; 4];
    memory.read(0xfffe, &mut bytes).unwrap();
    assert_eq!(&bytes, b"ABCD");
    memory.read_segment(&ds, 0xffff, &mut bytes[..1]).unwrap();
    assert_eq!(bytes[0], b'B');
    memory.write_segment(&ds, 0xfffe, b"XY").unwrap();
    memory.read(0xfffe, &mut bytes).unwrap();
    assert_eq!(&bytes, b"XYCD");
    memory.read_segment(&ds, 0xffff, &mut []).unwrap();
    memory.write_segment(&ds, 0xffff, &[]).unwrap();
}

#[test]
fn real_mode_segment_transfers_allow_cs_writes_and_preserve_rom_policy() {
    let map = PhysicalMemoryMap::new([
        (
            0x12000..=0x12fff,
            PhysicalMapping::Ram { backing_offset: 0 },
        ),
        (
            0x13000..=0x13fff,
            PhysicalMapping::Rom {
                backing_offset: 0x1000,
            },
        ),
    ])
    .unwrap();
    let (config, mut storage) = configuration(map);
    let mut memory = MemoryView::new(&config, MemoryAccess::from_bytes(&mut storage));
    let cs = SegmentRegister::real_mode(Segment::Cs, 0x1234);
    memory.write_backing(0x1000, b"CD").unwrap();
    memory.write_segment(&cs, 0xcbe, b"ABxy").unwrap();
    let mut bytes = [0; 4];
    memory.read_segment(&cs, 0xcbe, &mut bytes).unwrap();
    assert_eq!(&bytes, b"ABCD"); // CS:0CBE translates to 12FFE, crossing into ROM.
}

#[test]
fn invalid_spans_fail_before_any_transfer() {
    let (config, mut storage) = configuration(PhysicalMemoryMap::new([]).unwrap());
    let mut memory = MemoryView::new(&config, MemoryAccess::from_bytes(&mut storage));
    assert_eq!(memory.backing_size(), 0x3000);
    memory.write_backing(0x2fff, &[0x42]).unwrap();
    assert_eq!(
        memory.write_backing(0x2fff, &[1, 2]),
        Err(MemoryError::BackingRange)
    );
    let mut byte = [0];
    memory.read_backing(0x2fff, &mut byte).unwrap();
    assert_eq!(byte, [0x42]);
    let mut bytes = [0x55; 2];
    assert_eq!(
        memory.read(u32::MAX, &mut bytes),
        Err(MemoryError::AddressRange)
    );
    assert_eq!(bytes, [0x55; 2]);
    assert_eq!(
        memory.write(u32::MAX, &[1, 2]),
        Err(MemoryError::AddressRange)
    );
    assert_eq!(memory.read_u16(u32::MAX), Err(MemoryError::AddressRange));
    assert_eq!(
        memory.write_u16(u32::MAX, 0x1234),
        Err(MemoryError::AddressRange)
    );
    memory.read(u32::MAX, &mut byte).unwrap();
    assert_eq!(byte, [0xff]);
}

#[test]
fn overflowing_scalars_do_not_wrap_into_low_ram() {
    let map =
        PhysicalMemoryMap::new([(0..=0xfff, PhysicalMapping::Ram { backing_offset: 0 })]).unwrap();
    let (config, mut storage) = configuration(map);
    let mut memory = MemoryView::new(&config, MemoryAccess::from_bytes(&mut storage));
    memory.write_u64(0, 0xfedc_ba98_7654_3210).unwrap();
    assert_eq!(
        memory.write_u32(u32::MAX - 1, 0),
        Err(MemoryError::AddressRange)
    );
    assert_eq!(
        memory.write_u64(u32::MAX - 3, 0),
        Err(MemoryError::AddressRange)
    );
    assert_eq!(
        memory.read_u32(u32::MAX - 1),
        Err(MemoryError::AddressRange)
    );
    assert_eq!(
        memory.read_u64(u32::MAX - 3),
        Err(MemoryError::AddressRange)
    );
    assert_eq!(memory.read_u64(0).unwrap(), 0xfedc_ba98_7654_3210);
    assert_eq!(memory.read_u8(u32::MAX).unwrap(), 0xff);
}
