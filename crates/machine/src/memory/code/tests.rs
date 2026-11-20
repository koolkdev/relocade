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

fn register(memory: &Memory, source: &CodeSource) -> Arc<InstalledCode> {
    source.lock_current_bytes(memory).unwrap().register(source)
}

#[test]
fn aliases_share_speculative_bytes_and_only_installed_sources_are_indexed() {
    let memory = setup();
    memory.view().write(0x100, &[1, 2, 3, 4, 5]).unwrap();
    let mut reader = memory.code_reader();
    assert_eq!(reader.read(0x100, 5).unwrap(), [1, 2, 3, 4, 5]);
    memory.view().write(0x100100, &[9]).unwrap();
    assert_eq!(reader.read(0x100100, 3).unwrap(), [1, 2, 3]);
    let candidate = reader.finish([(0x100, 2), (0x100101, 2)]);
    assert!(candidate.ranges().eq([0x100..0x102, 0x100101..0x100103]));
    assert!(candidate
        .backing_ranges()
        .eq([0x1100..0x1102, 0x1101..0x1103]));
    assert!(memory.code_sources_at(0x1100).is_empty());
    assert!(matches!(
        candidate.lock_current_bytes(&memory),
        Err(Error::StaleCompilation)
    ));
    memory.view().write(0x100, &[1]).unwrap();
    memory.view().write(0x103, &[9]).unwrap(); // read-ahead
    let installed = register(&memory, &candidate);
    assert!(installed.physical_pages().eq([0]));
    let observers = memory.installed_code_at(0x1101);
    assert_eq!(observers.len(), 1);
    assert!(Arc::ptr_eq(&installed, &observers[0]));
    assert!(memory.code_sources_at(0x1103).is_empty());
    memory.view().write_backing(0x1800, &[1]).unwrap();
    memory.view().write(0x100100, &[1, 2, 3, 9]).unwrap();
    assert!(!installed.was_modified());

    memory.view().write(0x100102, &[8]).unwrap();
    assert!(installed.was_modified());
    installed.unregister();
    assert!(memory.installed_code_at(0x1101).is_empty());

    // After the first retirement, restoring the bytes permits another installation.
    memory.view().write(0x102, &[3]).unwrap();
    let retry = register(&memory, &candidate);
    memory.view().write_backing(0x1102, &[8]).unwrap();
    assert!(retry.was_modified());
    retry.unregister();

    // Restoring them again cannot publish a reply covering the twice-patched byte.
    memory.view().write(0x102, &[3]).unwrap();
    assert!(matches!(
        candidate.lock_current_bytes(&memory),
        Err(Error::StaleCompilation)
    ));
    assert!(observers[0].was_modified());
    assert!(!candidate.belongs_to(&setup()));
}

#[test]
fn a_cross_page_write_marks_only_the_installation_with_changed_bytes() {
    let memory = setup();
    memory.view().write(0xffe, &[1, 2, 3, 4]).unwrap();
    let install_span = |address, len| {
        let mut reader = memory.code_reader();
        reader.read(0xffe, 4).unwrap();
        register(&memory, &reader.finish([(address, len)]))
    };
    let first = install_span(0xffe, 3);
    let second = install_span(0x1000, 2);
    let writer = memory.clone();
    writer.view().write_backing(0x1fff, &[2, 3, 4]).unwrap();
    assert!(!memory.take_retirement_request());
    writer.view().write_backing(0x1fff, &[2, 3, 7]).unwrap();
    assert!(memory.take_retirement_request());
    assert!(!memory.take_retirement_request());
    assert!(!first.was_modified());
    assert!(second.was_modified());

    writer.view().write_backing(0x1ffe, &[8]).unwrap();
    assert!(memory.take_retirement_request());
    assert!(first.was_modified());
}

#[test]
fn blacklist_keeps_bytes_that_invalidated_two_successive_installations() {
    let memory = setup();
    let mut reader = memory.code_reader();
    reader.read(0xffe, 5).unwrap();
    let source = reader.finish([(0xffe, 5), (0x100fff, 3)]);
    let first = register(&memory, &source);
    let overlapping = register(&memory, &source);

    // One A20 write changes bytes on both pages, covered by both installations and their aliases.
    memory.view().write(0x100fff, &[1, 0, 2]).unwrap();
    assert!(first.was_modified());
    assert!(overlapping.was_modified());
    // Further writes see only dirty sources, including a byte the first write never touched.
    memory.view().write_backing(0x1fff, &[2]).unwrap();
    memory.view().write_backing(0x2002, &[3]).unwrap();
    first.unregister();
    overlapping.unregister();
    memory.view().write_backing(0x1ffe, &[4]).unwrap();

    let before_retry = memory.code.pages.lock().unwrap().excluded().clone();
    let mut reader = memory.code_reader();
    assert_eq!(reader.read(0x100ffe, 5).unwrap(), [4, 2, 0, 2, 3]);
    let retry = register(&memory, &reader.finish([(0x100ffe, 5)]));
    memory.view().write_backing(0x1ffe, &[0; 5]).unwrap();
    assert!(retry.was_modified());

    assert!(before_retry.is_empty());
    assert!(matches!(
        source.lock_current_bytes(&memory),
        Err(Error::StaleCompilation)
    ));
    let pages = memory.code.pages.lock().unwrap();
    let patched = pages.excluded();
    for byte in 0x1ffe..=0x2003 {
        assert_eq!(patched.intersects(byte, 1), matches!(byte, 0x1fff | 0x2001));
    }
}
