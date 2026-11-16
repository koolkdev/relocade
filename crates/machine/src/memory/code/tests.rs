use super::*;
use crate::MachineConfig;
use futures_lite::future::{block_on, poll_once};
use futures_util::FutureExt;
use relocade_x86::{PhysicalMapping, PhysicalMemoryMap};
use std::{
    future::Future,
    pin::pin,
    task::{Context, Waker},
};

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
    let reader = memory.code_reader();
    assert_eq!(
        block_on(reader.read([(0x100, 5)])).unwrap(),
        [[1, 2, 3, 4, 5]]
    );
    memory.clone().view().write(0x100100, &[9]).unwrap();
    assert_eq!(block_on(reader.read([(0x100100, 3)])).unwrap(), [[1, 2, 3]]);
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
    let reader = memory.code_reader();
    block_on(reader.read([(0x100, 1)])).unwrap();
    let source = reader.finish([(0x100, 1)]);
    memory.view().write(0x1100, &[1]).unwrap();
    assert!(!source.has_recorded_writes());
    memory.view().write_backing(0x1800, &[1]).unwrap();
    assert!(source.has_recorded_writes());
    let reader = memory.code_reader();
    block_on(reader.read([(0x100, 1)])).unwrap();
    let replacement = reader.finish([(0x100, 1)]);
    assert!(!replacement.has_recorded_writes());
    assert_eq!(memory.code_sources_at(0x1100).len(), 2);
    assert!(!source.belongs_to(&setup()));
}

#[test]
fn code_reads_wait_for_active_cpus_before_copying_new_pages() {
    let memory = setup();
    memory.view().write(0xfff, &[1, 2]).unwrap();
    let member = memory.execution.member();
    let active = member.try_enter().unwrap();
    let reader = memory.code_reader();
    let mut cx = Context::from_waker(Waker::noop());
    {
        let mut capture = pin!(reader.read([(0xfff, 2)]));
        // The first poll lets other reads queue their pages.
        assert!(capture.as_mut().poll(&mut cx).is_pending());
        assert!(capture.as_mut().poll(&mut cx).is_pending());
        assert!(member.try_enter().is_none());
        memory.view().write(0xfff, &[3, 4]).unwrap();
        drop(active);
        let bytes = block_on(capture).unwrap();
        assert_eq!(bytes, [[3, 4]]);
    }
    assert!(member.try_enter().is_some(), "capture released the pause");
}

#[test]
fn batched_reads_wake_every_waiter_for_shared_pages() {
    let memory = setup();
    let ranges: Vec<_> = (0..64)
        .map(|index| ((index % 2) * 0x1000 + index, 1))
        .collect();
    for (value, &(address, _)) in ranges.iter().enumerate() {
        memory.view().write(address, &[value as u8]).unwrap();
    }
    let reader = memory.code_reader();
    let bytes = block_on(reader.read(ranges)).unwrap();
    assert_eq!(bytes, (0..64).map(|value| vec![value]).collect::<Vec<_>>());
}

#[test]
fn retained_pages_can_be_captured_again_while_a_cpu_is_active() {
    let memory = setup();
    let reader = memory.code_reader();
    block_on(reader.read([(0x100, 1)])).unwrap();
    let source = reader.finish([(0x100, 1)]);
    let member = memory.execution.member();
    let _active = member.try_enter().unwrap();
    memory.view().write(0x100, &[42]).unwrap();
    let reader = memory.code_reader();
    {
        let mut read = pin!(reader.read([(0x100, 1)]));
        // Allow the batch to gather; it must then finish without pausing the CPU.
        assert!(block_on(poll_once(read.as_mut())).is_none());
        let bytes = read
            .now_or_never()
            .expect("retained pages do not pause CPUs")
            .unwrap();
        assert_eq!(bytes, [[42]]);
    }
    assert!(source.has_recorded_writes());
    assert!(!reader.finish([(0x100, 1)]).has_recorded_writes());
}

#[test]
fn page_markers_follow_captures_and_retained_sources() {
    let memory = setup();
    let original = memory.code_memory_map().1.to_bytes();
    let reader = memory.code_reader();
    block_on(reader.read([(0xfff, 2)])).unwrap();
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
