//! Shared PS/2 controller and keyboard, connected to the PC interrupt controllers.

mod controller;
mod keyboard;
mod scancodes;

use controller::Controller;
use relocade_frontend::keyboard::{self as frontend, KeyboardEvents, KeyboardInput};
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

/// Consumes one keyboard byte through the guest bus, leaving auxiliary data intact.
pub fn read_scan_code(io: &IoBus) -> Option<u8> {
    (io.read_port(STATUS_COMMAND_PORT, 1) & u32::from(OUTPUT_BUFFER_FULL | AUXILIARY_OUTPUT)
        == u32::from(OUTPUT_BUFFER_FULL))
    .then(|| io.read_port(DATA_PORT, 1) as u8)
}

/// Registers the controller ports and its asynchronous firmware and keyboard.
/// Host text is ignored: physical positions become scan bytes, with guest repeat.
/// The controller connects IRQ1 low until an output byte is ready.
pub async fn connect<C: Clock + Clone + 'static>(
    runner: &Runner<C>,
    pic: &Pic,
) -> (KeyboardInput, DeviceConnection) {
    let (input, events) = frontend::channel();
    let device = Rc::new(RefCell::new(Ps2Controller {
        controller: Rc::new(Controller::new(pic, [controller::Port::Keyboard])),
        startup: Some((runner.clock().clone(), events)),
    }));
    (input, runner.connect(device, [], PORTS).await)
}

/// Port callbacks touch registers only. Both firmware tasks run on the same owner.
struct Ps2Controller<C> {
    controller: Rc<Controller>,
    startup: Option<(C, KeyboardEvents)>,
}

impl<C: Clock> Device for Ps2Controller<C> {
    async fn run(device: Rc<RefCell<Self>>) {
        let (controller, clock, events) = {
            let mut device = device.borrow_mut();
            let (clock, events) = device.startup.take().expect("one keyboard activity");
            (device.controller.clone(), clock, events)
        };
        futures_util::future::join(controller.run(), keyboard::run(clock, events, &controller))
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
