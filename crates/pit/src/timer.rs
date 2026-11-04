//! Default channel-0 IRQ activity. BIOS owns the interrupt handler and clock data.

use crate::{clocks_at, time_at};
use relocade_machine::{Clock, Device, DeviceConnection, Runner};
use std::{
    cell::RefCell,
    rc::Rc,
    task::{Context, Poll, Waker},
};

const CLOCKS_PER_TICK: u64 = 65_536;

/// Registers a timer at the BIOS default rate and returns its bus connection.
/// The first period and its wait are armed before this returns. Divisor 65,536 at
/// approximately 1.193182 MHz supplies vector 08h every 54.9254 ms, without a PIC.
/// Unacknowledged edges coalesce; delayed wakes keep phase without replaying
/// missed edges. Programmable channel-0 ports are not implemented.
pub async fn connect_channel0<C: Clock + Clone + 'static>(runner: &Runner<C>) -> DeviceConnection {
    let timer = Rc::new(RefCell::new(Timer {
        clock: runner.clock().clone(),
        pending: false,
        waker: None,
    }));
    runner.connect(timer, [], []).await
}

struct Timer<C> {
    clock: C,
    pending: bool,
    waker: Option<Waker>,
}

impl<C: Clock + Clone> Device for Timer<C> {
    async fn run(device: Rc<RefCell<Self>>) {
        let clock = device.borrow().clock.clone();
        let origin = clock.now();
        let mut next_tick = CLOCKS_PER_TICK;
        loop {
            clock
                .wait_until(origin.saturating_add(time_at(next_tick)))
                .await;
            let elapsed = clocks_at(clock.now().saturating_sub(origin));
            next_tick = (elapsed / CLOCKS_PER_TICK + 1) * CLOCKS_PER_TICK;
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

#[cfg(test)]
mod tests;
