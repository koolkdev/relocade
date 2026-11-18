use super::*;
use crate::{MachineConfig, Memory};
use futures_util::FutureExt;
use relocade_x86::{PhysicalMapping, PhysicalMemoryMap};
use std::{
    future::Future,
    pin::pin,
    task::{Context, Waker},
};

fn setup() -> Memory {
    Memory::new(
        MachineConfig::new(
            0x2000,
            PhysicalMemoryMap::new([(0..=0x1fff, PhysicalMapping::Ram { backing_offset: 0 })])
                .unwrap(),
        )
        .unwrap(),
    )
    .unwrap()
}

#[test]
fn captured_pages_remain_readable_while_another_page_waits_for_cpus() {
    let memory = setup();
    memory.view().write(0x100, &[42]).unwrap();
    let pages = Pages::new(&memory);
    let page = pages.get(0x100).now_or_never().unwrap();
    let member = memory.execution.member();
    let active = member.try_enter().unwrap();
    let mut pending = pin!(pages.get(0x1000));
    let mut cx = Context::from_waker(Waker::noop());
    assert!(pending.as_mut().poll(&mut cx).is_pending());
    assert!(member.try_enter().is_none());

    memory.view().write(0x100, &[99]).unwrap();
    let again = pages
        .get(0)
        .now_or_never()
        .expect("captured pages never wait");
    assert!(Rc::ptr_eq(&page, &again));
    assert_eq!(again.bytes[0x100], 42);
    drop(active);
    assert!(pending.poll(&mut cx).is_ready());
    assert!(member.try_enter().is_some());
}
