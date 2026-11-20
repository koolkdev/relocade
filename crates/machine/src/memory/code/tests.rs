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
    let candidate = reader.finish([(0x100, &[2][..]), (0x100101, &[2][..])]);
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

    // A restored snapshot can be installed again with an independent write history.
    memory.view().write(0x102, &[3]).unwrap();
    let replacement = register(&memory, &candidate);
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
        register(&memory, &reader.finish([(address, &[len][..])]))
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
fn changed_instruction_extends_across_discontiguous_backing_pages() {
    use relocade_x86::{
        BlockSnapshot, CodeAddress, Compiler, ExecutionProfile, SegmentDefaultSize,
    };
    let map = PhysicalMemoryMap::new([
        (
            0..=0xfff,
            PhysicalMapping::Ram {
                backing_offset: 0x2000,
            },
        ),
        (0x1000..=0x1fff, PhysicalMapping::Ram { backing_offset: 0 }),
    ])
    .unwrap();
    let memory = Memory::new(MachineConfig::new(0x3000, map).unwrap()).unwrap();
    // NOP; MOV AX,7; NOP; RET. Only MOV crosses the page boundary.
    memory
        .view()
        .write(0xffd, &[0x90, 0xb8, 7, 0, 0x90, 0xc3])
        .unwrap();
    let mut reader = memory.code_reader();
    let bytes = reader.read(0xffd, 6).unwrap();
    let blocks = Compiler::new(ExecutionProfile::Real16)
        .compile_blocks(&[BlockSnapshot {
            address: CodeAddress::RealMode {
                segment: 0,
                eip: 0xffd,
            },
            code_default_size: SegmentDefaultSize::Bits16,
            bytes: &bytes,
            instruction_limit: 4,
        }])
        .unwrap();
    let source = reader.finish(
        blocks
            .block_instructions()
            .map(|(address, lengths)| (address.eip(), lengths)),
    );
    assert!(source.ranges().eq([0xffd..0x1000, 0x1000..0x1003]));
    let from_second_page = source
        .changed_instruction_ranges(1, 0, &[1, 0x90, 0xc3])
        .unwrap();
    assert!(from_second_page.eq([0x2ffe..0x3000, 0..1]));
    let from_first_page = source
        .changed_instruction_ranges(0, 0x2ffd, &[0x90, 0xb8, 9])
        .unwrap();
    assert!(from_first_page.eq([0x2ffe..0x3000, 0..1]));
}

#[test]
fn a_recompiled_instruction_shares_its_retry_across_aliases_and_block_boundaries() {
    let memory = setup();
    let mut reader = memory.code_reader();
    reader.read(0x100, 8).unwrap();
    let original = reader.finish([(0x100, &[1, 3, 1, 2, 1][..])]);
    let mut reader = memory.code_reader();
    reader.read(0x100101, 6).unwrap();
    let alias = reader.finish([(0x100101, &[3, 1, 2][..])]);
    let first = register(&memory, &original);
    let overlapping = register(&memory, &alias);

    // One write changes two instructions and leaves the instruction between them intact.
    memory.view().write(0x101, &[1, 0, 0, 0, 2]).unwrap();
    assert!(first.was_modified() && overlapping.was_modified());
    memory.view().write_backing(0x1101, &[3]).unwrap();
    let before_retry = memory.code.pages.lock().unwrap().excluded().clone();
    assert!(before_retry.is_empty());
    first.unregister();
    overlapping.unregister();

    let mut reader = memory.code_reader();
    reader.read(0x100100, 8).unwrap();
    let split = reader.finish([(0x100100, &[1, 3][..]), (0x100104, &[1, 2, 1][..])]);
    let retry = register(&memory, &split);
    // Change different offsets of both instructions in the replacement.
    memory.view().write(0x103, &[4, 0, 2, 5]).unwrap();
    assert!(retry.was_modified());
    let excluded = memory.code.pages.lock().unwrap().excluded().clone();
    assert!(before_retry.is_empty());
    for offset in 0..8 {
        assert_eq!(
            excluded.intersects(0x1100 + offset, 1),
            matches!(offset, 1..=3 | 5..=6)
        );
    }
    retry.unregister();

    memory.view().write(0x100, &[0; 8]).unwrap();
    assert!(matches!(
        original.lock_current_bytes(&memory),
        Err(Error::StaleCompilation)
    ));
    let mut reader = memory.code_reader();
    reader.read(0x104, 1).unwrap();
    let stable = reader.finish([(0x104, &[1][..])]);
    register(&memory, &stable).unregister();
}

#[test]
fn a_later_write_before_retirement_counts_the_other_patched_instruction() {
    let memory = setup();
    let capture = || {
        let mut reader = memory.code_reader();
        reader.read(0x100, 2).unwrap();
        reader.finish([(0x100, &[1, 1][..])])
    };
    let original = register(&memory, &capture());
    memory.view().write(0x100, &[1]).unwrap();
    memory.view().write(0x101, &[1]).unwrap();
    original.unregister();

    let retry = register(&memory, &capture());
    memory.view().write(0x101, &[2]).unwrap();
    retry.unregister();
    let pages = memory.code.pages.lock().unwrap();
    assert!(pages.excluded().intersects(0x1101, 1));
    assert!(!pages.excluded().intersects(0x1100, 1));
}
