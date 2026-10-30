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
    waker: RefCell<Option<Waker>>,
}
impl ManualClock {
    pub fn set(&self, now: Duration) {
        assert!(now >= self.now.get());
        self.now.set(now);
        if let Some(waker) = self.waker.borrow_mut().take() {
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
                *self.waker.borrow_mut() = Some(cx.waker().clone());
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

// Finish a cooperative yield before inspecting the device's next wait.
pub fn poll_video(
    mut video: std::pin::Pin<&mut impl std::future::Future<Output = ()>>,
    cx: &mut std::task::Context<'_>,
) {
    for _ in 0..2 {
        assert!(video.as_mut().poll(cx).is_pending());
    }
}

#[path = "../support/mod.rs"]
pub mod support;
pub use support::fixture;

pub fn text_vga<C: Clock>(clock: std::rc::Rc<C>) -> relocade_vga::Vga<C> {
    let mut vga = relocade_vga::Vga::with_clock(clock);
    fixture::initialize_font(&mut vga);
    vga
}
