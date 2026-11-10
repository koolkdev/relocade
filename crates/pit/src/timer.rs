//! PC timer registers and IRQ0 activity. BIOS owns the handler and clock data.

use crate::{clocks_at, time_at, Counter, CHANNEL_0_PORT, CHANNEL_2_PORT, CONTROL_PORT};
use event_listener::Event;
use futures_lite::future::or;
use relocade_machine::{yield_now, Clock, Device, DeviceConnection, Runner, Timestamp};
use relocade_pic::{IrqLine, Pic, TIMER_IRQ};
use std::{cell::RefCell, rc::Rc};

/// Channel-2 state at an exact point on the PIT's supplied clock.
#[derive(Clone, Debug)]
pub struct Channel2State {
    pub at: Timestamp,
    pub counter: Counter,
    /// Fraction of an input clock already elapsed, in billionths. The counter
    /// has consumed all whole input clocks before this fraction.
    pub fraction: u32,
}

/// Execution-owned PIT registers and programmable channel-0 interrupt.
/// Channel 2 shares its gate and output with the PC speaker.
pub struct Pit {
    clock: Clock,
    origin: Timestamp,
    at: Timestamp,
    channel0: Counter,
    channel2: Counter,
    channel2_observer: Option<Box<dyn FnMut(Channel2State)>>,
    irq: IrqLine,
    changed: Event,
}

impl Pit {
    /// Creates the timer on the runner's clock, connects IRQ0 and starts its activity.
    /// Returns the shared PIT and its register connection for the CPU bus.
    pub async fn initialize(runner: &Runner, pic: &Pic) -> (Rc<RefCell<Self>>, DeviceConnection) {
        let pit = Rc::new(RefCell::new(Self::with_clock(runner.clock().clone(), pic)));
        let connection = runner.connect(pit.clone(), [], [crate::PORT_RANGE]).await;
        (pit, connection)
    }

    /// Starts channel 0 in BIOS mode 3 with divisor 65,536, with OUT0 high.
    pub fn with_clock(clock: Clock, pic: &Pic) -> Self {
        let mut channel0 = Counter::default();
        channel0.set_gate(true);
        channel0.control(0x36);
        channel0.write(0);
        channel0.write(0);
        channel0.advance(1); // BIOS initialization has already loaded the divisor.
        let irq = pic.irq(TIMER_IRQ, channel0.output());
        let origin = clock.floor(clock.now());
        Self {
            origin,
            at: origin,
            clock,
            channel0,
            channel2: Counter::default(),
            channel2_observer: None,
            irq,
            changed: Event::new(),
        }
    }

    /// The timebase shared by hardware access and dependent devices.
    pub fn clock(&self) -> &Clock {
        &self.clock
    }

    pub fn channel2_output(&mut self) -> bool {
        self.sync_to_clock();
        self.channel2.output()
    }

    /// Applies the speaker gate and records the phase at this control write.
    pub fn set_channel2_gate(&mut self, high: bool) {
        self.sync_to_clock();
        self.channel2.set_gate(high);
        self.notify_channel2();
    }

    /// Replaces the channel-2 observer and supplies its initial state now.
    /// Every programming/gate write is observed synchronously, including writes
    /// at the same timestamp. Reads and latches do not notify. The observer must
    /// not borrow this PIT again while handling the notification.
    pub fn observe_channel2(&mut self, mut observer: impl FnMut(Channel2State) + 'static) {
        self.sync_to_clock();
        observer(self.channel2_snapshot());
        self.channel2_observer = Some(Box::new(observer));
    }

    fn channel2_snapshot(&self) -> Channel2State {
        Channel2State {
            at: self.at,
            counter: self.channel2.clone(),
            fraction: (self.clock.duration_since(self.at, self.origin).as_nanos()
                * u128::from(crate::INPUT_HZ)
                % 1_000_000_000) as u32,
        }
    }

    fn notify_channel2(&mut self) {
        let state = self.channel2_snapshot();
        if let Some(observer) = &mut self.channel2_observer {
            observer(state);
        }
    }

    fn sync_to_clock(&mut self) {
        let now = self.clock.now();
        let clocks = clocks_at(self.clock.duration_since(now, self.origin))
            - clocks_at(self.clock.duration_since(self.at, self.origin));
        if self
            .channel0
            .next_rising_edge()
            .is_some_and(|edge| edge <= clocks)
        {
            // Preserve one pending edge even if a delayed wake skipped whole
            // periods. The final level also serves a level-triggered PIC.
            self.irq.pulse();
        }
        self.channel0.advance(clocks);
        self.irq.set_level(self.channel0.output());
        self.channel2.advance(clocks);
        self.at = now;
    }
}

impl Device for Pit {
    async fn run(device: Rc<RefCell<Self>>) {
        let clock = device.borrow().clock.clone();
        device.borrow_mut().sync_to_clock();
        loop {
            let (deadline, changed) = {
                let device = device.borrow();
                (
                    device.channel0.next_transition().map(|clocks| {
                        clock.add(
                            device.origin,
                            time_at(
                                clocks_at(clock.duration_since(device.at, device.origin)) + clocks,
                            ),
                        )
                    }),
                    device.changed.listen(),
                )
            };
            if let Some(deadline) = deadline {
                or(clock.sleep_until(deadline), changed).await;
            } else {
                changed.await;
            }
            device.borrow_mut().sync_to_clock();
            // Divisor 2 can leave every deadline overdue. Give the CPU a turn
            // even then; elapsed edges still coalesce at the next synchronization.
            yield_now().await;
        }
    }

    fn read_port(&mut self, port: u16, bytes: u8) -> u32 {
        self.sync_to_clock();
        match (port, bytes) {
            (CHANNEL_0_PORT, 1) => self.channel0.read().into(),
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
        let channel = match port {
            CHANNEL_0_PORT => 0,
            CHANNEL_2_PORT => 2,
            CONTROL_PORT if value >> 6 == 3 => {
                // Read-back selection bits are high-active; requests are low-active.
                if value & 1 == 0 {
                    for (bit, counter) in [(2, &mut self.channel0), (8, &mut self.channel2)] {
                        if value & bit != 0 {
                            counter.latch(value & 0x20 == 0, value & 0x10 == 0);
                        }
                    }
                }
                return;
            }
            CONTROL_PORT => value >> 6,
            _ => return,
        };
        let counter = match channel {
            0 => &mut self.channel0,
            2 => &mut self.channel2,
            _ => return,
        };
        if port == CONTROL_PORT {
            counter.control(value);
            if value & 0x30 == 0 {
                return;
            }
        } else {
            counter.write(value);
        }
        if channel == 0 {
            self.irq.set_level(self.channel0.output());
            self.changed.notify(1);
        } else {
            self.notify_channel2();
        }
    }
}

#[cfg(test)]
mod tests;
