//! Fixed BIOS-rate timer activity. Clock data is updated by the IRQ0 handler.

use relocade_machine::{Clock, Device, DeviceConnection, Runner};
use std::{
    cell::RefCell,
    rc::Rc,
    task::{Context, Poll, Waker},
    time::Duration,
};

const PERIOD: Duration = Duration::from_nanos(65_536 * 1_000_000_000 / 1_193_182);

/// Registers a timer at the BIOS default rate and returns its bus connection.
/// The first period starts when the runner polls the activity. Divisor 65,536 at
/// approximately 1.193182 MHz supplies vector 08h every 54.9254 ms, without a PIC.
/// Unacknowledged edges coalesce; delayed wakes keep phase without replaying
/// missed edges. Programmable PIT ports and other channels are not implemented.
pub fn connect<C: Clock + Clone + 'static>(runner: &Runner<C>) -> DeviceConnection {
    let timer = Rc::new(RefCell::new(Timer {
        clock: runner.clock().clone(),
        pending: false,
        waker: None,
    }));
    runner.connect(timer, [], [])
}

struct Timer<C> {
    clock: C,
    pending: bool,
    waker: Option<Waker>,
}

impl<C: Clock + Clone> Device for Timer<C> {
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
