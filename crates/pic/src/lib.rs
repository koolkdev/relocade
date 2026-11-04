//! PC master 8259A interrupt arbitration, independent of CPU interrupt entry.

mod controller;

use controller::{Controller, IRQ_COUNT, OCW2_EOI};
use relocade_machine::{Clock, Device, DeviceConnection, IoBus, Runner};
use std::{
    cell::RefCell,
    ops::RangeInclusive,
    rc::Rc,
    task::{Context, Poll},
};

pub const COMMAND_PORT: u16 = 0x20;
pub const MASK_PORT: u16 = 0x21;
pub const PORT_RANGE: RangeInclusive<u16> = COMMAND_PORT..=MASK_PORT;
/// PC master interrupt wiring.
pub const TIMER_IRQ: u8 = 0;
pub const KEYBOARD_IRQ: u8 = 1;
const BIOS_VECTOR_BASE: u8 = 0x08;

/// Finishes the highest-priority active interrupt through the guest device bus.
pub fn end_of_interrupt(io: &IoBus) {
    io.write_port(COMMAND_PORT, 1, u32::from(OCW2_EOI));
}

/// One execution-owned master PIC with eight interrupt inputs. Clones share
/// arbitration and guest-visible registers. The machine polls only this device;
/// peripherals signal its [`IrqLine`] handles instead of supplying CPU vectors.
#[derive(Clone)]
pub struct Pic(Rc<RefCell<Controller>>);

impl Default for Pic {
    /// BIOS-initialized PC master vector base, with every IRQ masked.
    fn default() -> Self {
        Self::new(BIOS_VECTOR_BASE)
    }
}

impl Pic {
    /// Creates the BIOS-configured master and registers its ports with the runner.
    /// Returns a shared controller handle and its connection for the CPU bus.
    pub async fn initialize<C: Clock>(runner: &Runner<C>) -> (Self, DeviceConnection) {
        let pic = Self::default();
        let connection = runner
            .connect(Rc::new(RefCell::new(pic.clone())), [], [PORT_RANGE])
            .await;
        (pic, connection)
    }

    /// Starts in 8086 edge-triggered mode, with all inputs masked. The vector base
    /// must be aligned to eight; DOS uses 08h and unmasks its connected devices.
    pub fn new(vector_base: u8) -> Self {
        assert_eq!(vector_base % IRQ_COUNT, 0, "PIC vector base alignment");
        Self(Rc::new(RefCell::new(Controller::new(vector_base))))
    }

    /// Enables delivery on a connected line without dropping pending requests.
    pub fn unmask_irq(&self, number: u8) {
        assert!(number < IRQ_COUNT, "master PIC IRQ number");
        self.update(|state| state.mask &= !(1 << number));
    }

    pub fn irq(&self, number: u8) -> IrqLine {
        assert!(number < IRQ_COUNT, "master PIC IRQ number");
        IrqLine {
            pic: self.clone(),
            number,
        }
    }

    fn update(&self, change: impl FnOnce(&mut Controller)) {
        let waker = {
            let mut state = self.0.borrow_mut();
            change(&mut state);
            state.next_irq().and_then(|_| state.waker.take())
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

/// An IRQ input on the execution owner. A high transition latches an edge even
/// while masked; lowering it rearms the input. Repeated high levels do not create
/// additional edges. In level mode the asserted level remains eligible after EOI.
#[derive(Clone)]
pub struct IrqLine {
    pic: Pic,
    number: u8,
}

impl IrqLine {
    /// Establishes a peripheral's initial input level without creating an edge.
    /// Existing pending edges are preserved; level-triggered inputs still reflect
    /// the supplied level. Use [`Self::set_level`] for subsequent changes.
    pub fn set_initial_level(&self, high: bool) {
        self.pic
            .update(|state| state.set_initial_level(self.number, high));
    }

    pub fn set_level(&self, high: bool) {
        self.pic.update(|state| state.set_level(self.number, high));
    }

    /// Produces one low/high/low pulse, coalescing with an already pending edge.
    pub fn pulse(&self) {
        self.set_level(false);
        self.set_level(true);
        self.set_level(false);
    }
}

impl Device for Pic {
    fn read_port(&mut self, port: u16, bytes: u8) -> u32 {
        if bytes != 1 {
            return u32::MAX;
        }
        let mut state = self.0.borrow_mut();
        match port {
            COMMAND_PORT => state.read_command().into(),
            MASK_PORT => state.mask.into(),
            _ => u32::MAX,
        }
    }

    fn write_port(&mut self, port: u16, bytes: u8, value: u32) {
        if bytes != 1 {
            return;
        }
        self.update(|state| match port {
            COMMAND_PORT => state.command(value as u8),
            MASK_PORT => state.data(value as u8),
            _ => {}
        });
    }

    fn poll_interrupt(&mut self, cx: &mut Context<'_>) -> Poll<u8> {
        let mut state = self.0.borrow_mut();
        state.waker = Some(cx.waker().clone());
        match state.acknowledge() {
            Some(irq) => Poll::Ready(state.vector_base | irq),
            None => Poll::Pending,
        }
    }
}

#[cfg(test)]
mod tests;
