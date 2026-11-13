//! AdLib ports, timer status and sound synthesis, independent of platform output.
#![forbid(unsafe_code)]

mod opl2;
pub use opl2::Opl2;
mod timer;

use relocade_machine::{
    Clock, Device, DeviceCallbacks, DeviceConnection, PortRead, PortWrite, Runner, Timestamp,
};
use std::{cell::RefCell, rc::Rc};
use timer::Timer;

/// Write the register address; read timer flags and the OPL2 identification bits.
pub const ADDRESS_STATUS_PORT: u16 = 0x388;
/// Write the selected register. Reads return open bus.
pub const DATA_PORT: u16 = 0x389;

const TIMER_1_REGISTER: u8 = 0x02;
const TIMER_2_REGISTER: u8 = 0x03;
const TIMER_CONTROL_REGISTER: u8 = 0x04;

/// Guest-visible AdLib state on the execution clock.
/// Timer flags are updated on access; classic AdLib has no connected PIC IRQ.
pub struct Adlib {
    clock: Clock,
    address: u8,
    registers: [u8; 256],
    timers: [Timer; 2],
}

impl Adlib {
    pub fn new(clock: Clock) -> Self {
        Self {
            clock,
            address: 0,
            registers: [0; 256],
            timers: [Timer::new(80), Timer::new(320)],
        }
    }

    /// Connects the address/status and data ports on the existing runner.
    pub async fn initialize(runner: &Runner) -> Rc<RefCell<Self>> {
        let adlib = Rc::new(RefCell::new(Self::new(runner.clock().clone())));
        runner
            .connect(adlib.clone(), |io| Self::configure_io(io, &adlib))
            .await;
        adlib
    }

    pub fn configure_io(io: &mut DeviceConnection<'_>, adlib: &Rc<RefCell<Self>>) {
        let callbacks = DeviceCallbacks::new(adlib);
        io.bind(
            ADDRESS_STATUS_PORT,
            &PortRead::byte(callbacks.read(|_, adlib| adlib.read_status())),
            &PortWrite::byte(callbacks.write(|_, adlib, value| adlib.address = value)),
        );
        io.bind(
            DATA_PORT,
            &PortRead::byte(|_| 0xff),
            &PortWrite::byte(callbacks.write(|_, adlib, value| adlib.write_data(value))),
        );
    }

    fn update_timers(&mut self) -> Timestamp {
        let now = self.clock.now();
        for timer in &mut self.timers {
            timer.update(&self.clock, now);
        }
        now
    }

    fn read_status(&mut self) -> u8 {
        self.update_timers();
        let flags =
            (u8::from(self.timers[0].overflow) << 6) | (u8::from(self.timers[1].overflow) << 5);
        // Bits 1 and 2 distinguish OPL2 from OPL3 during hardware detection.
        0x06 | flags | if flags != 0 { 0x80 } else { 0 }
    }

    fn write_data(&mut self, value: u8) {
        let now = self.update_timers();
        match self.address {
            TIMER_1_REGISTER => self.timers[0].reload = value,
            TIMER_2_REGISTER => self.timers[1].reload = value,
            TIMER_CONTROL_REGISTER => {
                if value & 0x80 != 0 {
                    // Reset is a strobe: the remaining control bits are ignored.
                    for timer in &mut self.timers {
                        timer.overflow = false;
                    }
                } else {
                    self.timers[0].control(&self.clock, now, value & 1 != 0, value & 0x40 != 0);
                    self.timers[1].control(&self.clock, now, value & 2 != 0, value & 0x20 != 0);
                }
            }
            _ => self.registers[usize::from(self.address)] = value,
        }
    }
}

impl Device for Adlib {}
