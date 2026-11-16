use super::*;
use crate::MachineConfig;
use relocade_x86::{PhysicalMapping, PhysicalMemoryMap};

fn setup() -> Memory {
    let map = PhysicalMemoryMap::new([(
        0..=0x1fff,
        PhysicalMapping::Ram {
            backing_offset: 0x1000,
        },
    )])
    .unwrap();
    Memory::new(MachineConfig::new(0x3000, map).unwrap()).unwrap()
}

#[test]
fn aliases_share_capture_versions_and_sources_retain_only_emitted_bytes() {
    let memory = setup();
    memory.view().write(0x100, &[1, 2, 3, 4, 5]).unwrap();
    let mut reader = memory.code_reader();
    assert_eq!(reader.read(0x100, 5).unwrap(), [1, 2, 3, 4, 5]);
    memory.clone().view().write(0x100100, &[9]).unwrap();
    assert_eq!(reader.read(0x100100, 3).unwrap(), [1, 2, 3]);
    let source = reader.finish([(0x100, 3)]);
    assert!(source.ranges().eq(std::iter::once(0x100..0x103)));
    assert!(source.backing_ranges().eq(std::iter::once(0x1100..0x1103)));
    assert!(source.has_recorded_writes());
    assert_eq!(memory.code_sources_at(0x1101).len(), 1);
    assert!(memory.code_sources_at(0x1103).is_empty());
    let retained = source.clone();
    drop(source);
    assert_eq!(memory.code_sources_at(0x1101).len(), 1);
    drop(retained);
    assert!(memory.code_sources_at(0x1101).is_empty());
}

#[test]
fn versions_follow_tracked_pages_and_leave_other_pages_alone() {
    let memory = setup();
    let mut reader = memory.code_reader();
    reader.read(0x100, 1).unwrap();
    let source = reader.finish([(0x100, 1)]);
    memory.view().write(0x1100, &[1]).unwrap();
    assert!(!source.has_recorded_writes());
    memory.view().write_backing(0x1800, &[1]).unwrap();
    assert!(source.has_recorded_writes());
    let mut reader = memory.code_reader();
    reader.read(0x100, 1).unwrap();
    let replacement = reader.finish([(0x100, 1)]);
    assert!(!replacement.has_recorded_writes());
    assert_eq!(memory.code_sources_at(0x1100).len(), 2);
    assert!(!source.belongs_to(&setup()));
}

#[test]
fn page_markers_follow_captures_and_retained_sources() {
    let memory = setup();
    let original = memory.code_memory_map().1.to_bytes();
    let mut reader = memory.code_reader();
    reader.read(0xfff, 2).unwrap();
    let captured_revision = memory.code_revision();
    let captured = memory.code_memory_map().1.to_bytes();
    assert_eq!(&captured[..4], &5u32.to_le_bytes());
    assert_eq!(&captured[8..12], &5u32.to_le_bytes());

    let source = reader.finish([(0xfff, 1)]);
    assert!(memory.code_revision() > captured_revision);
    let source_map = memory.code_memory_map().1.to_bytes();
    assert_eq!(&source_map[..4], &5u32.to_le_bytes());
    assert_eq!(
        &source_map[8..12],
        &1u32.to_le_bytes(),
        "read-ahead is released"
    );

    let retained = source.clone();
    drop(source);
    assert_eq!(memory.code_memory_map().1.to_bytes(), source_map);
    drop(retained);
    assert_eq!(memory.code_memory_map().1.to_bytes(), original);
}
