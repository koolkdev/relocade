//! INT 15h/C2h firmware for the standard three-byte PS/2 pointing device.

mod callback;
mod interface;
mod services;

use interface::Interface;
use relocade_machine::{Clock, CpuContext, FarPointer};
use relocade_ps2::Controller;
use std::{cell::Cell, rc::Rc};

#[derive(Clone, Copy)]
#[repr(u8)]
enum Error {
    InvalidFunction = 1,
    InvalidParameter = 2,
    Interface = 3,
    Resend = 4,
    NoHandler = 5,
}

/// BIOS mouse configuration, packet assembly and guest callback registration.
/// Retains the same controller connected to the CPU's I/O bus.
pub struct Mouse<C> {
    interface: Interface<C>,
    callback: Cell<Option<FarPointer>>,
}

impl<C: Clock> Mouse<C> {
    pub fn new(clock: C, controller: Rc<Controller>) -> Self {
        Self {
            interface: Interface::new(clock, controller),
            callback: Cell::new(None),
        }
    }

    pub(crate) async fn interrupt(&self, call: &mut CpuContext<'_>) {
        if let Some(packet) = self.interface.packet(call.io) {
            if let Some(target) = self.callback.get() {
                callback::invoke(call, target, packet).await;
            }
        }
    }
}
