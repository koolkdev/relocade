//! Default channel-0 IRQ activity. BIOS owns the interrupt handler and clock data.

use crate::{clocks_at, time_at};
use relocade_machine::{Clock, Device, DeviceConnection, Runner};
use relocade_pic::IrqLine;
use std::{cell::RefCell, rc::Rc};

const CLOCKS_PER_TICK: u64 = 65_536;

/// Registers a timer at the BIOS default rate and returns its bus connection.
/// The first period and its wait are armed before this returns. Divisor 65,536 at
/// approximately 1.193182 MHz pulses the supplied IRQ line every 54.9254 ms.
/// Unacknowledged edges coalesce; delayed wakes keep phase without replaying
/// missed edges. Programmable channel-0 ports are not implemented.
pub async fn connect_channel0<C: Clock + Clone + 'static>(
    runner: &Runner<C>,
    irq: IrqLine,
) -> DeviceConnection {
    let timer = Rc::new(RefCell::new(Timer {
        clock: runner.clock().clone(),
        irq,
    }));
    runner.connect(timer, [], []).await
}

struct Timer<C> {
    clock: C,
    irq: IrqLine,
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
            device.borrow().irq.pulse();
        }
    }
}

#[cfg(test)]
mod tests;
