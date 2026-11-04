//! PC timer registers and IRQ0 activity. BIOS owns the handler and clock data.

use crate::{clocks_at, time_at, Counter, CHANNEL_2_PORT, CONTROL_PORT};
use relocade_machine::{Clock, Device, DeviceConnection, Runner, TimerHandle};
use relocade_pic::IrqLine;
use std::{cell::RefCell, rc::Rc, time::Duration};

const CLOCKS_PER_TICK: u64 = 65_536;

/// Execution-owned PIT registers and the default-rate channel-0 interrupt.
/// Channel 2 shares its gate and output with the PC speaker.
pub struct Pit<C = TimerHandle> {
    clock: Rc<C>,
    at: Duration,
    channel2: Counter,
    channel2_update: Option<Counter>,
    irq: IrqLine,
}

impl<C: Clock> Pit<C> {
    /// Creates the timer on the runner's clock and starts its IRQ activity.
    /// Returns the shared PIT and its register connection for the CPU bus.
    pub async fn initialize(
        runner: &Runner<C>,
        irq: IrqLine,
    ) -> (Rc<RefCell<Self>>, DeviceConnection)
    where
        C: Clone + 'static,
    {
        let pit = Rc::new(RefCell::new(Self::with_clock(
            Rc::new(runner.clock().clone()),
            irq,
        )));
        let connection = runner.connect(pit.clone(), [], [crate::PORT_RANGE]).await;
        (pit, connection)
    }

    pub fn with_clock(clock: Rc<C>, irq: IrqLine) -> Self {
        Self {
            at: clock.now(),
            clock,
            channel2: Counter::default(),
            channel2_update: None,
            irq,
        }
    }

    /// Current channel-2 state for starting playback at the machine clock.
    pub fn channel2(&mut self) -> Counter {
        self.sync_to_clock();
        self.channel2.clone()
    }

    pub fn channel2_output(&mut self) -> bool {
        self.sync_to_clock();
        self.channel2.output()
    }

    /// Applies the speaker gate and records the phase at this control write.
    pub fn set_channel2_gate(&mut self, high: bool) {
        self.sync_to_clock();
        self.channel2.set_gate(high);
        self.channel2_update = Some(self.channel2.clone());
    }

    /// Takes the latest programming state for the single speaker playback owner.
    /// Reads and latches leave this state untouched; writes may coalesce.
    pub fn take_channel2_update(&mut self) -> Option<Counter> {
        self.channel2_update.take()
    }

    fn sync_to_clock(&mut self) {
        let now = self.clock.now();
        self.channel2.advance(clocks_at(now) - clocks_at(self.at));
        self.at = now;
    }
}

impl<C: Clock> Device for Pit<C> {
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

    fn read_port(&mut self, port: u16, bytes: u8) -> u32 {
        self.sync_to_clock();
        match (port, bytes) {
            (CHANNEL_2_PORT, 1) => self.channel2.read().into(),
            _ => u32::MAX,
        }
    }

    fn write_port(&mut self, port: u16, bytes: u8, value: u32) {
        self.sync_to_clock();
        if bytes != 1 {
            return;
        }
        let value = value as u8;
        match port {
            CHANNEL_2_PORT => self.channel2.write(value),
            CONTROL_PORT if value >> 6 == 2 => {
                self.channel2.control(value);
                if value & 0x30 == 0 {
                    return; // Count latching does not change playback.
                }
            }
            // Read-back selects channel 2 with bit 3 high. Requests are low-active.
            CONTROL_PORT if value >> 6 == 3 && value & 9 == 8 => {
                self.channel2.latch(value & 0x20 == 0, value & 0x10 == 0);
                return;
            }
            _ => return,
        }
        self.channel2_update = Some(self.channel2.clone());
    }
}

#[cfg(test)]
mod tests;
