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
    Duration::from_nanos((count * 1_000_000_000).div_ceil(frequency))
}
