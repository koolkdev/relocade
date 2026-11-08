//! Shared PS/2 controller, keyboard and mouse on the PC interrupt controllers.

mod controller;
mod keyboard;
mod mouse;
pub mod protocol;
mod scancodes;

pub use controller::Controller;
use controller::Port;
use relocade_frontend::{
    keyboard::{self as frontend, KeyboardEvents, KeyboardInput},
    mouse::{MouseEvents, MouseInput},
};
use relocade_machine::{Clock, Device, DeviceConnection, IoBus, Runner};
use relocade_pic::Pic;
use std::{cell::RefCell, ops::RangeInclusive, rc::Rc};

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

/// Registers the shared controller, keyboard and three-button mouse firmware.
/// Host text is ignored: physical positions become scan bytes, with guest repeat.
/// IRQ1/IRQ12 start low; the auxiliary interface and mouse reporting start disabled.
pub async fn connect<C: Clock + Clone + 'static>(
    runner: &Runner<C>,
    pic: &Pic,
) -> (Inputs, Rc<Controller>, DeviceConnection) {
    let (keyboard, events) = frontend::channel();
    let (mouse, mouse_events) = relocade_frontend::mouse::channel();
    let controller = Rc::new(Controller::new(pic, [Port::Keyboard, Port::Auxiliary]));
    let device = Rc::new(RefCell::new(Ps2Controller {
        controller: controller.clone(),
        startup: Some((runner.clock().clone(), events, mouse_events)),
    }));
    (
        Inputs { keyboard, mouse },
        controller,
        runner.connect(device, [], PORTS).await,
    )
}

/// Port callbacks touch registers only. All firmware tasks run on the same owner.
struct Ps2Controller<C> {
    controller: Rc<Controller>,
    startup: Option<(C, KeyboardEvents, MouseEvents)>,
}

impl<C: Clock + Clone> Device for Ps2Controller<C> {
    async fn run(device: Rc<RefCell<Self>>) {
        let (controller, clock, events, mouse_events) = {
            let mut device = device.borrow_mut();
            let (clock, events, mouse_events) = device.startup.take().expect("one PS/2 activity");
            (device.controller.clone(), clock, events, mouse_events)
        };
        futures_util::future::join3(
            controller.run(),
            keyboard::run(clock.clone(), events, &controller),
            mouse::run(clock, mouse_events, &controller),
        )
        .await;
    }

    fn read_port(&mut self, port: u16, bytes: u8) -> u32 {
        if bytes != 1 {
            return u32::MAX;
        }
        match port {
            DATA_PORT => self.controller.read_data().into(),
            STATUS_COMMAND_PORT => self.controller.status().into(),
            _ => u32::MAX,
        }
    }

    fn write_port(&mut self, port: u16, bytes: u8, value: u32) {
        if bytes == 1 {
            self.controller.write(port, value as u8);
        }
    }
}

#[cfg(test)]
mod tests;
