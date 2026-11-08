//! INT 15h/C2h firmware for the standard three-byte PS/2 pointing device.

mod interface;
mod services;

use interface::Interface;
use relocade_machine::Clock;
use relocade_ps2::Controller;
use std::rc::Rc;

#[derive(Clone, Copy)]
#[repr(u8)]
enum Error {
    InvalidFunction = 1,
    InvalidParameter = 2,
    Interface = 3,
    Resend = 4,
    NoHandler = 5,
}

/// BIOS pointing-device configuration over the shared PS/2 controller.
/// Retains the same controller connected to the CPU's I/O bus.
pub struct Mouse<C> {
    interface: Interface<C>,
}

impl<C: Clock> Mouse<C> {
    pub fn new(clock: C, controller: Rc<Controller>) -> Self {
        Self {
            interface: Interface::new(clock, controller),
        }
    }
}
