//! PC timer channel 0 at the BIOS default rate, delivered through the device bus.
//!
//! The fixed divisor is 65,536 at approximately 1.193182 MHz: one IRQ0 every
//! 54.9254 ms. The device supplies the standard real-mode vector 08h. Register it
//! with `runner.connect(pit, [], [])`; programmable ports and other channels are
//! not implemented yet.
#![forbid(unsafe_code)]

use relocade_machine::{Clock, Device, TimerHandle};
use std::{
    cell::RefCell,
    rc::Rc,
    task::{Context, Poll, Waker},
    time::Duration,
};

const PERIOD: Duration = Duration::from_nanos(65_536 * 1_000_000_000 / 1_193_182);

/// Fixed-rate channel 0. The runner drives its timer activity alongside the CPU.
/// Unacknowledged timer edges coalesce into one pending IRQ, including while IF
/// is clear. Delayed host wakes keep the original timer phase without replaying
/// every elapsed edge.
pub struct Pit<C = TimerHandle> {
    clock: Rc<C>,
    pending: bool,
    waker: Option<Waker>,
}

impl<C: Clock> Pit<C> {
    pub fn with_clock(clock: Rc<C>) -> Self {
        Self {
            clock,
            pending: false,
            waker: None,
        }
    }
}

impl<C: Clock> Device for Pit<C> {
    async fn run(device: Rc<RefCell<Self>>) {
        let clock = device.borrow().clock.clone();
        let mut deadline = clock.now().saturating_add(PERIOD);
        loop {
            clock.wait_until(deadline).await;
            let now = clock.now();
            let remainder = now.saturating_sub(deadline).as_nanos() % PERIOD.as_nanos();
            deadline = now.saturating_add(PERIOD - Duration::from_nanos(remainder as u64));
            let waker = {
                let mut device = device.borrow_mut();
                device.pending = true;
                device.waker.take()
            };
            if let Some(waker) = waker {
                waker.wake();
            }
        }
    }

    fn poll_interrupt(&mut self, cx: &mut Context<'_>) -> Poll<u8> {
        if std::mem::take(&mut self.pending) {
            Poll::Ready(0x08)
        } else {
            self.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}
