//! The 8042's data and status/command registers, bound once at connection.

use crate::{Controller, DATA_PORT, STATUS_COMMAND_PORT};
use relocade_machine::{DeviceConnection, PortRead, PortWrite};
use std::rc::Rc;

pub(super) fn configure(io: &mut DeviceConnection<'_>, controller: &Rc<Controller>) {
    io.bind(
        DATA_PORT,
        &read(controller, Controller::read_data),
        &write(controller, Controller::write_data),
    );
    io.bind(
        STATUS_COMMAND_PORT,
        &read(controller, Controller::status),
        &write(controller, Controller::write_command),
    );
}

// The controller uses interior mutability and does not retain these operations.
fn read(controller: &Rc<Controller>, read: impl Fn(&Controller) -> u8 + 'static) -> PortRead {
    let controller = controller.clone();
    PortRead::byte(move |_| read(&controller))
}

fn write(controller: &Rc<Controller>, write: impl Fn(&Controller, u8) + 'static) -> PortWrite {
    let controller = controller.clone();
    PortWrite::byte(move |_, value| write(&controller, value))
}
