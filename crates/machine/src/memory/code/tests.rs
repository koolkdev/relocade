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

fn register(memory: &Memory, source: &CodeSource) -> (Arc<InstalledCode>, Vec<u32>) {
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
    let (installed, _) = register(&memory, &candidate);
    let observers = memory.installed_code_at(0x1101);
    assert_eq!(observers.len(), 1);
    assert!(Arc::ptr_eq(&installed, &observers[0]));
    assert!(memory.code_sources_at(0x1103).is_empty());
    memory.view().write_backing(0x1800, &[1]).unwrap();
    memory.view().write(0x100100, &[1, 2, 3, 9]).unwrap();
    assert!(!installed.was_modified());

    memory.view().write(0x100102, &[8]).unwrap();
    assert!(installed.was_modified());
    installed.unregister(|_| {});
    assert!(memory.installed_code_at(0x1101).is_empty());

    // A restored snapshot can be installed again with an independent write history.
    memory.view().write(0x102, &[3]).unwrap();
    let (replacement, _) = register(&memory, &candidate);
    assert!(!replacement.was_modified());
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
    let (first, _) = install_span(0xffe, 3);
    let (second, _) = install_span(0x1000, 2);
    memory.view().write_backing(0x1fff, &[2, 3, 7]).unwrap();
    assert!(!first.was_modified());
    assert!(second.was_modified());
}

#[test]
fn tracking_changes_only_on_the_first_and_last_installation() {
    let memory = setup();
    let mut reader = memory.code_reader();
    reader.read(0x100, 1).unwrap();
    reader.read(0x100100, 1).unwrap();
    let source = reader.finish([(0x100100, 1), (0x100, 1)]);
    assert!(source.backing_ranges().all(|span| span == (0x1100..0x1101)));

    let (first, added) = register(&memory, &source);
    assert_eq!(added, [0]);
    let (second, added) = register(&memory, &source);
    assert!(added.is_empty());
    let mut removed = Vec::new();
    first.unregister(|page| removed.push(page));
    assert!(removed.is_empty());
    second.unregister(|page| removed.push(page));
    assert_eq!(removed, [0]);
}
