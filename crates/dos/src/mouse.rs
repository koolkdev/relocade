//! Resident INT 33h state and the BIOS packet consumer, independent of InDOS.

mod cursor;
mod input;
mod motion;
mod services;
mod state;

use crate::state::DosState;
use relocade_machine::{
    real_mode_interrupt::{interrupt_handler, Interrupts},
    Clock, FarPointer, IoBus, MemoryView,
};
use state::State;
use std::{cell::RefCell, rc::Rc};

const SERVICE_ENTRY: FarPointer = FarPointer {
    segment: 0xf000,
    offset: 0x150,
};
const PACKET_ENTRY: FarPointer = FarPointer {
    segment: 0xf000,
    offset: 0x160,
};

#[derive(Default)]
pub(crate) struct Mouse {
    state: RefCell<State>,
    cursor: RefCell<cursor::Cursor>,
}

impl Mouse {
    /// The driver survives launches, but a program's far pointer does not.
    pub(crate) fn retire_callback(&self) {
        self.state.borrow_mut().callback = state::Callback::default();
    }

    pub(crate) fn end_process(&self, memory: &mut MemoryView<'_>, io: &IoBus) {
        self.retire_callback();
        self.cursor.borrow_mut().reset(memory, io);
    }

    fn update_cursor(&self, memory: &mut MemoryView<'_>, io: &IoBus) {
        let position = self.state.borrow().position(memory);
        self.cursor.borrow_mut().update(memory, io, position);
    }
}

pub(crate) fn register(interrupts: &mut Interrupts<'_>, session: Rc<DosState>, clock: Clock) {
    let driver = session.clone();
    interrupts.register_interrupt(
        crate::interrupts::MOUSE,
        SERVICE_ENTRY,
        interrupt_handler!(driver.mouse.dispatch),
    );
    interrupts.register_far_procedure(PACKET_ENTRY, async move |call| {
        session.mouse.receive(call, clock.now()).await;
    });
}
