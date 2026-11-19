//! PC timer registers and channel-0 transitions. BIOS owns the IRQ0 handler.

use crate::{Counter, CHANNEL_0_PORT, CHANNEL_2_PORT, CONTROL_PORT, INPUT_HZ};
use relocade_machine::{
    Alarm, Clock, Device, DeviceCallbacks, DeviceConnection, PortRead, PortWrite, Runner, Timestamp,
};
use relocade_pic::{IrqLine, Pic, TIMER_IRQ};
use std::{cell::RefCell, rc::Rc};

/// Channel-2 state at an observed native timestamp on the PIT's clock.
#[derive(Clone, Debug)]
pub struct Channel2State {
    pub at: Timestamp,
    pub counter: Counter,
    /// Numerator of the elapsed fraction of an input clock. The denominator is
    /// the supplied clock's ticks per second; whole input clocks are in `counter`.
    pub fraction: u64,
}

/// Execution-owned PIT registers and programmable channel-0 interrupt.
/// Channel 2 shares its gate and output with the PC speaker.
pub struct Pit {
    clock: Clock,
    updated: Timestamp,
    fraction: u64,
    channel0: Counter,
    channel2: Counter,
    channel2_observer: Option<Box<dyn FnMut(Channel2State)>>,
    irq: IrqLine,
    alarm: Option<Alarm>,
}

impl Pit {
    /// Connects IRQ0 and schedules channel-0 transitions on the runner's clock.
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
    /// [`Runner::connect`] schedules transitions on this clock. A standalone
    /// timer can be driven by calling [`Self::advance_to`] at each deadline.
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
            updated: started_at,
            fraction: 0,
            clock,
            channel0,
            channel2: Counter::default(),
            channel2_observer: None,
            irq,
            alarm: None,
        }
    }

    /// The clock shared by hardware access and dependent devices.
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
            fraction: self.fraction,
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
        self.schedule_transition();
    }

    fn schedule_transition(&self) {
        if let Some(alarm) = &self.alarm {
            alarm.replace(self.next_deadline());
        }
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
        self.advance_to(self.clock.now());
    }

    /// Materialize counter state at an observed native reading.
    pub fn advance_to(&mut self, now: Timestamp) {
        if now <= self.updated {
            return;
        }
        let ticks = now.ticks().wrapping_sub(self.updated.ticks());
        let rate = u128::from(self.clock.tick_rate().ticks_per_second().get());
        let span = u128::from(ticks) * u128::from(INPUT_HZ) + u128::from(self.fraction);
        let clocks = u64::try_from(span / rate).expect("PIT progress fits in input clocks");
        self.fraction = (span % rate) as u64;
        self.updated = now;
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
    /// Earliest native reading at or after the next channel-0 transition.
    pub fn next_deadline(&self) -> Option<Timestamp> {
        let clocks = self.channel0.next_transition()?;
        let rate = u128::from(self.clock.tick_rate().ticks_per_second().get());
        let ticks =
            (u128::from(clocks) * rate - u128::from(self.fraction)).div_ceil(u128::from(INPUT_HZ));
        (ticks <= i64::MAX as u128)
            .then(|| Timestamp::from_ticks(self.updated.ticks().wrapping_add(ticks as u64)))
    }
}

impl Device for Pit {
    async fn attach(device: &Rc<RefCell<Self>>, runner: &Runner) {
        let timer = Rc::downgrade(device);
        let mut pit = device.borrow_mut();
        pit.alarm = Some(runner.alarm_on(&pit.clock, move || {
            if let Some(timer) = timer.upgrade() {
                let mut pit = timer.borrow_mut();
                pit.sync_to_clock();
                pit.schedule_transition();
            }
        }));
        pit.sync_to_clock();
        pit.schedule_transition();
    }
}

#[cfg(test)]
mod tests;
