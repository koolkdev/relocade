mod support;

use relocade_machine::{
    ConfigurationError, Machine, MachineConfig, MappingError, MemoryError, PagedMemoryMap,
};
use relocade_x86::{Compiler, CpuState, Exception, SegmentProfile, StoredSegment};
use support::Storage;

fn config() -> MachineConfig {
    let mut map = PagedMemoryMap::default();
    map.map(0x400000..=0x400fff, 0x2000, true).unwrap();
    map.map(0x401000..=0x401fff, 0, false).unwrap();
    MachineConfig::new(
        Compiler::new(SegmentProfile::Flat32).target(),
        CpuState::default(),
        0x3000,
        map,
    )
    .unwrap()
}

#[test]
fn protected_pages_validate_capacity_and_alignment() {
    let mut map = PagedMemoryMap::default();
    assert_eq!(
        map.map(1..=0xfff, 0, true),
        Err(MappingError::UnalignedRange)
    );
    assert_eq!(
        map.map(0..=0xfff, 1, true),
        Err(MappingError::UnalignedRange)
    );
    map.map(0xfffff000..=u32::MAX, 0x2000, true).unwrap();
    assert!(matches!(
        MachineConfig::new(
            Compiler::new(SegmentProfile::Flat32).target(),
            CpuState::default(),
            0x2fff,
            map
        ),
        Err(ConfigurationError::InsufficientMemory)
    ));
}

#[test]
fn live_mappings_control_full_span_transfers_and_faults() {
    let config = config();
    let mut storage = Storage::new(&config);
    let mut machine = Machine::new(&config, &mut storage);
    let mut memory = machine.memory();
    memory.write_backing(0x2ffe, b"AB").unwrap();
    memory.write_backing(0, b"CD").unwrap();
    let mut bytes = [0; 4];
    memory.read(0x400ffe, &mut bytes).unwrap();
    assert_eq!(&bytes, b"ABCD");
    assert_eq!(
        memory.write(0x400ffe, b"WXYZ"),
        Err(MemoryError::PageFault(Exception::PageFault {
            linear_address: 0x401000,
            error_code: 3,
        }))
    );
    memory.read(0x400ffe, &mut bytes).unwrap();
    assert_eq!(&bytes, b"ABCD");
    memory.unmap_pages(0x401000..=0x401fff).unwrap();
    assert_eq!(
        memory.read(0x400ffe, &mut bytes),
        Err(MemoryError::PageFault(Exception::PageFault {
            linear_address: 0x401000,
            error_code: 0,
        }))
    );
    assert_eq!(&bytes, b"ABCD");
    assert_eq!(
        memory.write(0x400ffe, b"WXYZ"),
        Err(MemoryError::PageFault(Exception::PageFault {
            linear_address: 0x401000,
            error_code: 2,
        }))
    );
    memory.map_pages(0x401000..=0x401fff, 0x1000, true).unwrap();
    memory.write(0x400ffe, b"WXYZ").unwrap();
    memory.read_backing(0x1000, &mut bytes[..2]).unwrap();
    assert_eq!(&bytes[..2], b"YZ");
    assert_eq!(
        memory.map_pages(0x401000..=0x401fff, 0x3000, true),
        Err(MappingError::BackingRange)
    );
    memory.read(0x400ffe, &mut bytes).unwrap();
    assert_eq!(&bytes, b"WXYZ");
}

#[test]
fn protected_segment_permissions_and_expand_down_bounds_precede_paging() {
    let config = config();
    let mut storage = Storage::new(&config);
    let mut machine = Machine::new(&config, &mut storage);
    let mut memory = machine.memory();
    let mut segment = StoredSegment {
        base: 0x400000,
        limit: 0xfff,
        ..StoredSegment::flat_data32(0x23)
    };
    segment.readable = 0;
    let mut bytes = [0x55; 2];
    assert_eq!(
        memory.read_segment(&segment, 0x2000, &mut bytes),
        Err(MemoryError::SegmentAccess)
    );
    assert_eq!(bytes, [0x55; 2]);
    segment.readable = 1;
    segment.expand_down = 1;
    segment.big = 0;
    assert_eq!(
        memory.write_segment(&segment, 0xfff, b"AB"),
        Err(MemoryError::SegmentRange)
    );
    assert_eq!(
        memory.write_segment(&segment, 0xffff, b"AB"),
        Err(MemoryError::SegmentRange)
    );
    assert_eq!(
        memory.write_segment(&segment, 0x1000, b"AB"),
        Err(MemoryError::PageFault(Exception::PageFault {
            linear_address: 0x401000,
            error_code: 3,
        }))
    );
}
