//! The 8042's data and status/command registers, bound once at connection.

use crate::{Controller, DATA_PORT, STATUS_COMMAND_PORT};
use relocade_machine::{DeviceConnection, PortRead, PortWrite};
use std::rc::Rc;

pub(super) fn configure(io: &mut DeviceConnection<'_>, controller: &Rc<Controller>) {
    let data = io.port(DATA_PORT);
    let status = io.port(STATUS_COMMAND_PORT);
    data.set_read(io.context(), &read(controller, Controller::read_data));
    data.set_write(io.context(), &write(controller, Controller::write_data));
    status.set_read(io.context(), &read(controller, Controller::status));
    status.set_write(io.context(), &write(controller, Controller::write_command));
}

// These registers accept bytes only. Keep that policy shared while preserving
// each concrete operation in its Wasm callback.
fn read(controller: &Rc<Controller>, read: impl Fn(&Controller) -> u8 + 'static) -> PortRead {
    let controller = controller.clone();
    PortRead::new(move |_, _, bytes| {
        if bytes == 1 {
            read(&controller).into()
        } else {
            u32::MAX
        }
    })
}

fn write(controller: &Rc<Controller>, write: impl Fn(&Controller, u8) + 'static) -> PortWrite {
    let controller = controller.clone();
    PortWrite::new(move |_, _, bytes, value| {
        if bytes == 1 {
            write(&controller, value as u8);
        }
    })
}
