use super::*;
use crate::MachineConfig;
use futures_util::FutureExt;
use relocade_x86::{PhysicalMapping, PhysicalMemoryMap};

fn setup() -> Memory {
    let map = PhysicalMemoryMap::new([(
        0..=0x2fff,
        PhysicalMapping::Ram {
            backing_offset: 0x1000,
        },
    )])
    .unwrap();
    Memory::new(MachineConfig::new(0x4000, map).unwrap()).unwrap()
}

fn register(memory: &Memory, bytes: InstructionBytes) -> Result<CodeSource> {
    memory
        .register_code(bytes)
        .now_or_never()
        .expect("no CPU is active")
}

#[test]
fn discovery_reuses_backing_bytes_and_registers_only_emitted_instructions() {
    let memory = setup();
    memory.view().write(0x100, &[1, 2, 3, 4, 5]).unwrap();
    let mut reader = memory.code_reader();
    assert_eq!(reader.read(0x100, 5).unwrap(), [1, 2, 3, 4, 5]);
    memory.view().write(0x100103, &[9]).unwrap();
    assert_eq!(reader.read(0x100100, 5).unwrap(), [1, 2, 3, 4, 5]);
    let source = register(&memory, reader.finish([(0x100, 3)])).unwrap();
    assert!(source.ranges().eq(std::iter::once(0x100..0x103)));
    assert!(source.backing_ranges().eq(std::iter::once(0x1100..0x1103)));
    assert!(!source.has_recorded_writes());
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
    let source = register(&memory, reader.finish([(0x100, 1)])).unwrap();
    memory.view().write(0x1100, &[1]).unwrap();
    assert!(!source.has_recorded_writes());
    memory.view().write_backing(0x1800, &[1]).unwrap();
    assert!(source.has_recorded_writes());
    let mut reader = memory.code_reader();
    reader.read(0x100, 1).unwrap();
    let replacement = register(&memory, reader.finish([(0x100, 1)])).unwrap();
    assert!(!replacement.has_recorded_writes());
    assert_eq!(memory.code_sources_at(0x1100).len(), 2);
    assert!(!source.belongs_to(&setup()));
}

#[test]
fn changed_emitted_bytes_reject_the_entire_registration() {
    let memory = setup();
    let mut reader = memory.code_reader();
    reader.read(0xfff, 2).unwrap();
    let member = memory.execution.member();
    let active = member.try_enter().unwrap();
    let revision = memory.code_revision();
    let mut pending = std::pin::pin!(memory.register_code(reader.finish([(0xfff, 2)])));
    assert!(pending.as_mut().now_or_never().is_none());
    memory.view().write(0x101000, &[1]).unwrap(); // A20 alias of the second page.
    drop(active);
    assert!(matches!(
        pending.now_or_never(),
        Some(Err(Error::StaleCompilation))
    ));
    assert_eq!(memory.code_revision(), revision);
    assert!(memory.code_sources_at(0x1fff).is_empty());
    assert!(memory.code_sources_at(0x2000).is_empty());
}

#[test]
fn only_new_emitted_pages_require_a_stopped_boundary() {
    let memory = setup();
    let member = memory.execution.member();
    let active = member.try_enter().unwrap();
    let (revision, map) = memory.code_memory_map();
    let original = map.to_bytes();
    let emitted = || {
        let mut reader = memory.code_reader();
        reader.read(0xfff, 0x1002).unwrap(); // Three pages read; only two emitted.
        reader.finish([(0xfff, 2)])
    };
    let mut first = std::pin::pin!(memory.register_code(emitted()));
    assert_eq!(memory.code_memory_map().1.to_bytes(), original);
    assert!(first.as_mut().now_or_never().is_none());
    assert!(member.try_enter().is_none());
    let mut same = std::pin::pin!(memory.register_code(emitted()));
    assert!(same.as_mut().now_or_never().is_none());

    drop(active);
    let first = first.now_or_never().unwrap().unwrap();
    let _running = member.try_enter().unwrap();
    assert_eq!(
        memory.code_revision(),
        revision + 1,
        "both markers publish together"
    );
    let same = same
        .now_or_never()
        .expect("both pages were registered together")
        .unwrap();
    let mut expected = memory.configuration().memory_map().clone();
    expected.set_code_page(0x1000, true);
    expected.set_code_page(0x2000, true);
    assert_eq!(memory.code_memory_map().1.to_bytes(), expected.to_bytes());

    let mut reader = memory.code_reader();
    reader.read(0x2000, 1).unwrap();
    let mut other = std::pin::pin!(memory.register_code(reader.finish([(0x2000, 1)])));
    assert!(other.as_mut().now_or_never().is_none());
    let retained = memory
        .register_code(emitted())
        .now_or_never()
        .expect("tracked pages do not wait for unrelated cold pages")
        .unwrap();
    drop((first, same, retained));
    assert_eq!(memory.code_memory_map().1.to_bytes(), original);
}
