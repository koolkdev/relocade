//! Resident INT 33h state and the BIOS packet consumer, independent of InDOS.

mod coordinates;
mod cursor;
mod input;
mod motion;
mod services;
mod state;

use crate::state::DosState;
use coordinates::Coordinates;
use relocade_machine::{
    real_mode_interrupt::{interrupt_handler, Interrupts},
    Clock, FarPointer,
};
use relocade_vga::FramebufferAccess;
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

pub(crate) struct Mouse {
    video: Rc<RefCell<dyn FramebufferAccess>>,
    state: RefCell<State>,
    cursor: RefCell<cursor::Cursor>,
}

impl Mouse {
    pub(crate) fn new(video: Rc<RefCell<dyn FramebufferAccess>>) -> Self {
        let coordinates = Coordinates::new(video.borrow_mut().framebuffer().geometry());
        Self {
            video,
            state: RefCell::new(State::new(coordinates)),
            cursor: RefCell::default(),
        }
    }

    /// The driver survives launches, but a program's far pointer does not.
    pub(crate) fn retire_callback(&self) {
        self.state.borrow_mut().callback = state::Callback::default();
    }

    pub(crate) fn end_process(&self) {
        self.retire_callback();
        let mut video = self.video.borrow_mut();
        let mut frame = video.framebuffer();
        self.cursor.borrow_mut().reset(&mut frame);
    }

    fn coordinates(&self) -> Coordinates {
        Coordinates::new(self.video.borrow_mut().framebuffer().geometry())
    }

    fn update_cursor(&self) {
        let mut video = self.video.borrow_mut();
        let mut frame = video.framebuffer();
        let coordinates = Coordinates::new(frame.geometry());
        let position = self.state.borrow().position(coordinates);
        self.cursor
            .borrow_mut()
            .update(&mut frame, coordinates.drawing_position(position));
    }
}

pub(crate) fn register<C: Clock + 'static>(
    interrupts: &mut Interrupts<'_>,
    session: Rc<DosState>,
    clock: C,
) {
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
