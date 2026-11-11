//! The 8042's two independent byte registers, bound once at connection.

use crate::{Controller, DATA_PORT, STATUS_COMMAND_PORT};
use relocade_machine::{DeviceConnection, IoContext, PortPolicy};
use std::rc::Rc;

pub(super) fn configure(io: &mut DeviceConnection<'_>, controller: &Rc<Controller>) {
    io.port(
        DATA_PORT,
        PortPolicy::BYTE,
        read(controller, Controller::read_data),
        write(controller, Controller::write_data),
    );
    io.port(
        STATUS_COMMAND_PORT,
        PortPolicy::BYTE,
        read(controller, Controller::status),
        write(controller, Controller::write_command),
    );
}

fn read(
    controller: &Rc<Controller>,
    read: impl Fn(&Controller) -> u8 + 'static,
) -> impl Fn(IoContext<'_>) -> u8 + 'static {
    let controller = controller.clone();
    move |_| read(&controller)
}

fn write(
    controller: &Rc<Controller>,
    write: impl Fn(&Controller, u8) + 'static,
) -> impl Fn(IoContext<'_>, u8) + 'static {
    let controller = controller.clone();
    move |_, value| write(&controller, value)
}
