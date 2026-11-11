//! Adapts existing device connections to the shared port map.

use super::{ports::Ports, Device, DeviceConnection, IoContext, PortRead, PortWrite};
use std::{cell::RefCell, rc::Rc};

pub(super) fn register(
    connections: &[DeviceConnection],
    mappings: &Ports,
    context: &mut IoContext<'_>,
) {
    for connection in connections {
        let ports = mappings.device(context.bus, &connection.device);
        let read = PortRead::new(port_read(&connection.device));
        let write = PortWrite::new(port_write(&connection.device));
        for range in &connection.ports {
            ports.set_read(context.reborrow(), range.clone(), &read);
            ports.set_write(context.reborrow(), range.clone(), &write);
        }
    }
}

// Engine functions must not prolong a legacy device's lifetime beyond its bus.
fn port_read(device: &Rc<RefCell<dyn Device>>) -> impl Fn(IoContext<'_>, u16, u8) -> u32 + 'static {
    let device = Rc::downgrade(device);
    move |context, port, bytes| {
        device
            .upgrade()
            .expect("connected device")
            .borrow_mut()
            .read_port(context, port, bytes)
    }
}

fn port_write(device: &Rc<RefCell<dyn Device>>) -> impl Fn(IoContext<'_>, u16, u8, u32) + 'static {
    let device = Rc::downgrade(device);
    move |context, port, bytes, value| {
        device
            .upgrade()
            .expect("connected device")
            .borrow_mut()
            .write_port(context, port, bytes, value)
    }
}
