//! Shared PS/2 controller, keyboard and mouse on the PC interrupt controllers.

mod controller;
mod keyboard;
mod mouse;
mod ports;
pub mod protocol;
mod scancodes;

pub use controller::Controller;
use futures_util::{stream, StreamExt};
use relocade_frontend::{
    keyboard::{self as frontend, KeyboardEvent, KeyboardInput},
    mouse::{self as pointer, MouseEvent, MouseInput},
};
use relocade_machine::{IoBus, Runner};
use relocade_pic::Pic;
use std::{
    ops::RangeInclusive,
    rc::{Rc, Weak},
};

pub const DATA_PORT: u16 = 0x60;
pub const STATUS_COMMAND_PORT: u16 = 0x64;
pub const PORTS: [RangeInclusive<u16>; 2] = [
    DATA_PORT..=DATA_PORT,
    STATUS_COMMAND_PORT..=STATUS_COMMAND_PORT,
];
/// Port 64h: a byte is available for the guest to read from port 60h.
pub const OUTPUT_BUFFER_FULL: u8 = 1;
/// Port 64h: firmware has not yet consumed the guest's last port write.
pub const INPUT_BUFFER_FULL: u8 = 1 << 1;
/// Port 64h: the available output byte came from the auxiliary port.
pub const AUXILIARY_OUTPUT: u8 = 1 << 5;

/// Reads the retained output latch for IRQ1, leaving unread auxiliary data intact.
/// OBF need not be set: a guest hook may read the byte before chaining to BIOS.
pub fn read_keyboard_latch(io: &IoBus) -> Option<u8> {
    let status = io.read_port(STATUS_COMMAND_PORT, 1) as u8;
    (status & AUXILIARY_OUTPUT == 0).then(|| io.read_port(DATA_PORT, 1) as u8)
}

/// Consumes an unread auxiliary byte, leaving keyboard data and an empty latch alone.
pub fn read_auxiliary_byte(io: &IoBus) -> Option<u8> {
    let status = io.read_port(STATUS_COMMAND_PORT, 1) as u8;
    (status & (OUTPUT_BUFFER_FULL | AUXILIARY_OUTPUT) == OUTPUT_BUFFER_FULL | AUXILIARY_OUTPUT)
        .then(|| io.read_port(DATA_PORT, 1) as u8)
}

/// Host senders for the two PS/2 devices. Clones can cross execution workers.
#[derive(Clone)]
pub struct Inputs {
    pub keyboard: KeyboardInput,
    pub mouse: MouseInput,
}

/// Connects the controller's register operations and starts keyboard and mouse firmware.
/// Host text is ignored: physical positions become scan bytes, with guest repeat.
/// IRQ1/IRQ12 start low; the auxiliary interface and mouse reporting start disabled.
pub async fn connect(runner: &Runner, pic: &Pic) -> (Inputs, Rc<Controller>) {
    let clock = runner.clock().clone();
    let controller = Rc::new_cyclic(|weak: &Weak<Controller>| {
        let controller = weak.clone();
        let alarm = runner.alarm(move || {
            if let Some(controller) = controller.upgrade() {
                controller.service();
            }
        });
        Controller::new(
            clock.clone(),
            pic,
            Some(mouse::Mouse::new(clock)),
            Some(alarm),
        )
    });
    controller.connect_ports(&runner.io());
    let (keyboard, keys) = frontend::channel();
    let (mouse, motion) = pointer::channel();
    let keyboard_controller = controller.clone();
    runner.on_input(
        keys.chain(stream::iter([KeyboardEvent::FocusLost])),
        move |key| {
            keyboard_controller.key_event(key);
        },
    );
    let mouse_controller = controller.clone();
    runner.on_input(
        motion.chain(stream::iter([MouseEvent::FocusLost])),
        move |event| {
            mouse_controller.mouse_event(event);
        },
    );
    (Inputs { keyboard, mouse }, controller)
}

#[cfg(test)]
mod tests;
