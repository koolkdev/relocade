use super::*;
use crate::MachineConfig;
use futures_util::FutureExt;
use relocade_x86::{PhysicalMapping, PhysicalMemoryMap};
use std::{
    future::Future,
    pin::pin,
    task::{Context, Poll, Waker},
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
        reader.read([(0x100, 5)]).now_or_never().unwrap().unwrap(),
        [[1, 2, 3, 4, 5]]
    );
    memory.clone().view().write(0x100100, &[9]).unwrap();
    assert_eq!(
        reader
            .read([(0x100100, 3)])
            .now_or_never()
            .unwrap()
            .unwrap(),
        [[1, 2, 3]]
    );
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
    reader.read([(0x100, 1)]).now_or_never().unwrap().unwrap();
    let source = reader.finish([(0x100, 1)]);
    memory.view().write(0x1100, &[1]).unwrap();
    assert!(!source.has_recorded_writes());
    memory.view().write_backing(0x1800, &[1]).unwrap();
    assert!(source.has_recorded_writes());
    let reader = memory.code_reader();
    reader.read([(0x100, 1)]).now_or_never().unwrap().unwrap();
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
        assert!(capture.as_mut().poll(&mut cx).is_pending());
        assert!(member.try_enter().is_none());
        memory.view().write(0xfff, &[3, 4]).unwrap();
        drop(active);
        let Poll::Ready(Ok(bytes)) = capture.as_mut().poll(&mut cx) else {
            panic!("the active CPU reached its boundary");
        };
        assert_eq!(bytes, [[3, 4]]);
    }
    assert!(member.try_enter().is_some(), "capture released the pause");
}
