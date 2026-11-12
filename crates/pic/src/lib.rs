//! PC cascaded 8259A interrupt arbitration, independent of CPU interrupt entry.

mod controller;
mod pair;

use controller::{IRQ_COUNT, OCW2_EOI};
use pair::Pair;
use relocade_machine::{
    Device, DeviceCallbacks, DeviceConnection, IoBus, IoContext, PortRead, PortWrite, Runner,
};
use std::{
    cell::RefCell,
    ops::RangeInclusive,
    rc::Rc,
    task::{Context, Poll},
};

pub const COMMAND_PORT: u16 = 0x20;
pub const MASK_PORT: u16 = 0x21;
pub const PORT_RANGE: RangeInclusive<u16> = COMMAND_PORT..=MASK_PORT;
pub const SLAVE_COMMAND_PORT: u16 = 0xa0;
pub const SLAVE_MASK_PORT: u16 = 0xa1;
pub const PORTS: [RangeInclusive<u16>; 2] = [PORT_RANGE, SLAVE_COMMAND_PORT..=SLAVE_MASK_PORT];
/// PC interrupt wiring.
pub const TIMER_IRQ: u8 = 0;
pub const KEYBOARD_IRQ: u8 = 1;
pub const CASCADE_IRQ: u8 = 2;
pub const MOUSE_IRQ: u8 = 12;

/// Completes an IRQ through the guest bus, acknowledging a slave before its master.
/// The IRQ number identifies the PC wire, independently of guest vector remapping.
pub fn end_of_interrupt(io: &IoBus, irq: u8) {
    assert!(irq < 16, "PC IRQ number");
    if irq >= 8 {
        io.write_port(SLAVE_COMMAND_PORT, 1, u32::from(OCW2_EOI));
    }
    io.write_port(COMMAND_PORT, 1, u32::from(OCW2_EOI));
}

/// One execution-owned master/slave PIC pair with sixteen interrupt inputs. Clones share
/// arbitration and guest-visible registers. The machine polls only this device;
/// peripherals signal [`IrqLine`] handles instead of supplying CPU vectors.
#[derive(Clone)]
pub struct Pic(Rc<RefCell<Pair>>);

impl Default for Pic {
    /// BIOS-initialized PC vector bases (08h/70h), with every IRQ masked.
    fn default() -> Self {
        Self::new(0x08, 0x70)
    }
}

impl Pic {
    /// Creates the BIOS-configured pair and registers both chips with the runner.
    pub async fn initialize(runner: &Runner) -> Self {
        let pic = Self::default();
        let device = Rc::new(RefCell::new(pic.clone()));
        runner
            .connect(device.clone(), |io| Self::configure_io(io, &device))
            .await;
        pic
    }

    /// Bind the master and slave command/data operations. Both chips accept bytes only.
    pub fn configure_io(io: &mut DeviceConnection<'_>, device: &Rc<RefCell<Self>>) {
        let callbacks = DeviceCallbacks::new(device);
        io.bind(
            COMMAND_PORT,
            &PortRead::byte(callbacks.read(|_, pic| pic.update(|pair| pair.master.read_command()))),
            &PortWrite::byte(
                callbacks.write(|_, pic, value| pic.update(|pair| pair.master.command(value))),
            ),
        );
        io.bind(
            MASK_PORT,
            &PortRead::byte(callbacks.read(|_, pic| pic.update(|pair| pair.master.mask))),
            &PortWrite::byte(
                callbacks.write(|_, pic, value| pic.update(|pair| pair.master.data(value))),
            ),
        );
        io.bind(
            SLAVE_COMMAND_PORT,
            &PortRead::byte(callbacks.read(|_, pic| pic.update(Pair::read_slave_command))),
            &PortWrite::byte(
                callbacks.write(|_, pic, value| pic.update(|pair| pair.slave.command(value))),
            ),
        );
        io.bind(
            SLAVE_MASK_PORT,
            &PortRead::byte(callbacks.read(|_, pic| pic.update(|pair| pair.slave.mask))),
            &PortWrite::byte(
                callbacks.write(|_, pic, value| pic.update(|pair| pair.slave.data(value))),
            ),
        );
    }

    /// Starts in 8086 edge-triggered mode, with all inputs masked and the slave
    /// wired to master IRQ2. Each vector base must be aligned to eight.
    pub fn new(master_vector_base: u8, slave_vector_base: u8) -> Self {
        for base in [master_vector_base, slave_vector_base] {
            assert_eq!(base % IRQ_COUNT, 0, "PIC vector base alignment");
        }
        Self(Rc::new(RefCell::new(Pair::new(
            master_vector_base,
            slave_vector_base,
        ))))
    }

    /// Enables a line without dropping requests. Slave delivery also requires
    /// unmasking the master cascade input, just as with guest mask writes.
    pub fn unmask_irq(&self, number: u8) {
        assert!(number < 2 * IRQ_COUNT, "PC IRQ number");
        self.update(|state| state.chip(number).mask &= !(1 << (number % IRQ_COUNT)));
    }

    /// Connects a peripheral's IRQ line at its starting output level without
    /// inventing an edge or clearing pending requests. A high input is still
    /// eligible in level-triggered mode. Reuse or clone the returned line for
    /// runtime transitions.
    pub fn irq(&self, number: u8, initial_high: bool) -> IrqLine {
        assert!(number < 2 * IRQ_COUNT, "PC IRQ number");
        assert_ne!(number, CASCADE_IRQ, "IRQ2 is wired to the slave PIC");
        self.update(|state| {
            state
                .chip(number)
                .record_level(number % IRQ_COUNT, initial_high)
        });
        IrqLine {
            pic: self.clone(),
            number,
        }
    }

    fn update<T>(&self, change: impl FnOnce(&mut Pair) -> T) -> T {
        let (result, waker) = {
            let mut state = self.0.borrow_mut();
            let result = change(&mut state);
            state.refresh_cascade();
            let waker = state.master.next_irq().and_then(|_| state.waker.take());
            (result, waker)
        };
        if let Some(waker) = waker {
            waker.wake();
        }
        result
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
    pub fn set_level(&self, high: bool) {
        self.pic.update(|state| {
            state
                .chip(self.number)
                .set_level(self.number % IRQ_COUNT, high);
        });
    }

    /// Produces one low/high/low pulse, coalescing with an already pending edge.
    pub fn pulse(&self) {
        self.set_level(false);
        self.set_level(true);
        self.set_level(false);
    }
}

impl Device for Pic {
    fn poll_interrupt(&mut self, _context: IoContext<'_>, cx: &mut Context<'_>) -> Poll<u8> {
        let mut state = self.0.borrow_mut();
        state.waker = Some(cx.waker().clone());
        let vector = state.acknowledge();
        state.refresh_cascade();
        vector.map_or(Poll::Pending, Poll::Ready)
    }
}

#[cfg(test)]
mod tests;
