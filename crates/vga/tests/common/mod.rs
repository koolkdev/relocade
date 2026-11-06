#![allow(dead_code)]

use relocade_machine::{Clock, Device};
use std::{
    cell::{Cell, RefCell},
    future::poll_fn,
    task::{Poll, Waker},
    time::Duration,
};

#[derive(Default)]
pub struct ManualClock {
    now: Cell<Duration>,
    pub deadline: Cell<Option<Duration>>,
    wakers: RefCell<Vec<Waker>>,
}
impl ManualClock {
    pub fn set(&self, now: Duration) {
        assert!(now >= self.now.get());
        self.now.set(now);
        for waker in self.wakers.borrow_mut().drain(..) {
            waker.wake();
        }
    }
}
impl Clock for ManualClock {
    fn now(&self) -> Duration {
        self.now.get()
    }
    async fn wait_until(&self, deadline: Duration) {
        self.deadline.set(Some(deadline));
        poll_fn(|cx| {
            if self.now() >= deadline {
                Poll::Ready(())
            } else {
                let mut wakers = self.wakers.borrow_mut();
                if !wakers.iter().any(|waker| waker.will_wake(cx.waker())) {
                    wakers.push(cx.waker().clone());
                }
                Poll::Pending
            }
        })
        .await;
    }
}

pub fn crtc(device: &mut impl Device, index: u8, value: u8) {
    device.write_port(0x3d4, 2, u32::from(u16::from_le_bytes([index, value])));
}

pub fn dots(count: u64, frequency: u64) -> Duration {
    Duration::from_nanos((u128::from(count) * 1_000_000_000).div_ceil(u128::from(frequency)) as u64)
}

// Default hardware timing: 800 dots/line, 449 lines/frame, retrace at line 412.
pub fn boundary(frame: u64) -> Duration {
    dots((412 + (frame - 1) * 449) * 800, 25_175_000)
}

// Elapsed time since entering the fixed 640x480 DISPI mode.
pub fn svga_boundary(frame: u64) -> Duration {
    dots((490 + (frame - 1) * 525) * 800, 25_175_000)
}

// Service pending display work and register its next wait.
pub fn poll_video(
    video: std::pin::Pin<&mut impl std::future::Future<Output = ()>>,
    cx: &mut std::task::Context<'_>,
) {
    assert!(video.poll(cx).is_pending());
}

#[path = "../support/mod.rs"]
pub mod support;
pub use support::fixture;

pub fn text_vga<C: Clock + 'static>(clock: std::rc::Rc<C>) -> relocade_vga::Vga<C> {
    let mut vga = relocade_vga::Vga::with_clock(clock);
    fixture::initialize_font(&mut vga);
    vga
}
