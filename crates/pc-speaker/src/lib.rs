//! PC speaker hardware and synthesis.

use relocade_machine::{Clock, Device, TimerHandle};
use relocade_pit::{clocks_at, Counter, CHANNEL_2_PORT, CONTROL_PORT as PIT_CONTROL_PORT};
use std::{ops::RangeInclusive, rc::Rc, time::Duration};

/// PC/AT system control port B, including the speaker gate and data bits.
pub const SPEAKER_CONTROL_PORT: u16 = 0x61;
pub const PORTS: [RangeInclusive<u16>; 2] = [
    CHANNEL_2_PORT..=PIT_CONTROL_PORT,
    SPEAKER_CONTROL_PORT..=SPEAKER_CONTROL_PORT,
];

/// Execution-owned PIT channel 2 and speaker control port.
pub struct Speaker<C = TimerHandle> {
    clock: Rc<C>,
    at: Duration,
    counter: Counter,
    control: u8,
}

impl<C: Clock> Speaker<C> {
    pub fn with_clock(clock: Rc<C>) -> Self {
        Self {
            at: clock.now(),
            clock,
            counter: Counter::default(),
            control: 0,
        }
    }

    /// Digital speaker drive: [`SPEAKER_CONTROL_PORT`] bit 1 AND timer OUT2.
    pub fn output(&mut self) -> bool {
        self.sync_to_clock();
        self.control & 2 != 0 && self.counter.output()
    }

    fn sync_to_clock(&mut self) {
        let now = self.clock.now();
        self.counter.advance(clocks_at(now) - clocks_at(self.at));
        self.at = now;
    }
}

impl<C: Clock> Device for Speaker<C> {
    fn read_port(&mut self, port: u16, bytes: u8) -> u32 {
        self.sync_to_clock();
        if bytes != 1 {
            return u32::MAX;
        }
        match port {
            CHANNEL_2_PORT => self.counter.read().into(),
            SPEAKER_CONTROL_PORT => (self.control | (u8::from(self.counter.output()) << 5)).into(),
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
            CHANNEL_2_PORT => self.counter.write(value),
            PIT_CONTROL_PORT if value >> 6 == 2 => self.counter.control(value),
            // Read-back selects channel 2 with bit 3 high. Requests are low-active.
            PIT_CONTROL_PORT if value >> 6 == 3 && value & 9 == 8 => {
                self.counter.latch(value & 0x20 == 0, value & 0x10 == 0);
            }
            SPEAKER_CONTROL_PORT => {
                self.control = value & 3;
                self.counter.set_gate(value & 1 != 0);
            }
            _ => {}
        }
    }
}
