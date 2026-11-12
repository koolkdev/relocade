//! PC timer registers and IRQ0 activity. BIOS owns the handler and clock data.

use crate::{clocks_at, time_at, Counter, CHANNEL_0_PORT, CHANNEL_2_PORT, CONTROL_PORT};
use event_listener::Event;
use futures_lite::future::or;
use relocade_machine::{
    yield_now, Clock, Device, DeviceCallbacks, DeviceConnection, PortRead, PortWrite, Runner,
    Timestamp,
};
use relocade_pic::{IrqLine, Pic, TIMER_IRQ};
use std::{cell::RefCell, rc::Rc};

/// Channel-2 state at the PIT's consumed position on its supplied clock.
/// Whole nanoseconds are integrated from device initialization; a native
/// fraction remains pending. Snapshot times share that same phase for audio.
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
    started_at: Timestamp,
    updated: Timestamp,
    channel0: Counter,
    channel2: Counter,
    channel2_observer: Option<Box<dyn FnMut(Channel2State)>>,
    irq: IrqLine,
    changed: Event,
}

impl Pit {
    /// Creates the timer on the runner's clock, connects IRQ0 and starts its activity.
    pub async fn initialize(runner: &Runner, pic: &Pic) -> Rc<RefCell<Self>> {
        let pit = Rc::new(RefCell::new(Self::with_clock(runner.clock().clone(), pic)));
        runner
            .connect(pit.clone(), |io| Self::configure_io(io, &pit))
            .await;
        pit
    }

    /// Bind count and control operations with the PIT's byte-only transfer policy.
    pub fn configure_io(io: &mut DeviceConnection<'_>, pit: &Rc<RefCell<Self>>) {
        let callbacks = DeviceCallbacks::new(pit);
        let unused_read = PortRead::byte(callbacks.read(|_, pit| {
            pit.sync_to_clock();
            0xff
        }));
        io.bind(
            CHANNEL_0_PORT,
            &PortRead::byte(callbacks.read(|_, pit| pit.read_channel0())),
            &PortWrite::byte(callbacks.write(|_, pit, value| pit.write_channel0(value))),
        );
        io.bind(
            CHANNEL_0_PORT + 1,
            &unused_read,
            &PortWrite::byte(callbacks.write(|_, pit, _| pit.sync_to_clock())),
        );
        io.bind(
            CHANNEL_2_PORT,
            &PortRead::byte(callbacks.read(|_, pit| pit.read_channel2())),
            &PortWrite::byte(callbacks.write(|_, pit, value| pit.write_channel2(value))),
        );
        io.bind(
            CONTROL_PORT,
            &unused_read,
            &PortWrite::byte(callbacks.write(|_, pit, value| pit.write_control(value))),
        );
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
        let started_at = clock.now();
        Self {
            started_at,
            updated: started_at,
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
            at: self.updated,
            counter: self.channel2.clone(),
            fraction: (self
                .clock
                .duration_since(self.updated, self.started_at)
                .as_nanos()
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

    fn read_channel0(&mut self) -> u8 {
        self.sync_to_clock();
        self.channel0.read()
    }

    fn read_channel2(&mut self) -> u8 {
        self.sync_to_clock();
        self.channel2.read()
    }

    fn write_channel0(&mut self, value: u8) {
        self.sync_to_clock();
        self.channel0.write(value);
        self.notify_channel0();
    }

    fn write_channel2(&mut self, value: u8) {
        self.sync_to_clock();
        self.channel2.write(value);
        self.notify_channel2();
    }

    fn notify_channel0(&self) {
        self.irq.set_level(self.channel0.output());
        self.changed.notify(1);
    }

    fn write_control(&mut self, value: u8) {
        self.sync_to_clock();
        match value >> 6 {
            0 => {
                self.channel0.control(value);
                if value & 0x30 != 0 {
                    self.notify_channel0();
                }
            }
            2 => {
                self.channel2.control(value);
                if value & 0x30 != 0 {
                    self.notify_channel2();
                }
            }
            3 if value & 1 == 0 => {
                // Read-back selection bits are high-active; requests are low-active.
                for (bit, counter) in [(2, &mut self.channel0), (8, &mut self.channel2)] {
                    if value & bit != 0 {
                        counter.latch(value & 0x20 == 0, value & 0x10 == 0);
                    }
                }
            }
            _ => {}
        }
    }

    fn sync_to_clock(&mut self) {
        let previous = self.updated;
        self.clock
            .consume_elapsed(&mut self.updated, self.clock.now());
        let clocks = clocks_at(self.clock.duration_since(self.updated, self.started_at))
            - clocks_at(self.clock.duration_since(previous, self.started_at));
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
                            device.started_at,
                            time_at(
                                clocks_at(clock.duration_since(device.updated, device.started_at))
                                    + clocks,
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
}

#[cfg(test)]
mod tests;
