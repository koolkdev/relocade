//! Scoped device setup and adaptation of existing register methods.

use super::{
    ports::{PortRead, PortWrite},
    validate_ranges, Device, IoContext,
};
use std::{cell::RefCell, ops::RangeInclusive, rc::Rc};

pub(super) struct ConnectedDevice {
    pub(super) device: Rc<RefCell<dyn Device>>,
    pub(super) mmio: Vec<RangeInclusive<u32>>,
    pub(super) ports: Vec<RangeInclusive<u16>>,
}

/// Configures one device during [`super::IoBus::connect`] or [`crate::Runner::connect`].
/// Configuration finishes before the device's activity starts.
///
/// ```
/// use relocade_machine::{Device, IoBus, IoContext};
/// use std::{cell::RefCell, rc::Rc};
/// struct Registers;
/// impl Device for Registers {
///     fn read_port(&mut self, _: IoContext<'_>, _: u16, _: u8) -> u32 { 0x14 }
/// }
/// let io = IoBus::default();
/// io.connect(Rc::new(RefCell::new(Registers)), |io| {
///     io.map_ports(std::iter::once(0x64..=0x64));
/// });
/// assert_eq!(io.read_port(0x64, 1), 0x14);
/// ```
///
/// The setup scope cannot be retained by a device:
/// ```compile_fail
/// use relocade_machine::{Device, IoBus};
/// use std::{cell::RefCell, rc::Rc};
/// struct Registers;
/// impl Device for Registers {}
/// let mut saved = None;
/// IoBus::default().connect(Rc::new(RefCell::new(Registers)), |io| saved = Some(io));
/// ```
pub struct DeviceConnection<'a> {
    context: IoContext<'a>,
    connection: ConnectedDevice,
}

impl<'a> DeviceConnection<'a> {
    pub(super) fn new(context: IoContext<'a>, device: Rc<RefCell<dyn Device>>) -> Self {
        Self {
            context,
            connection: ConnectedDevice {
                device,
                mmio: Vec::new(),
                ports: Vec::new(),
            },
        }
    }

    /// Maps ranges to this device's existing `read_port` and `write_port` methods.
    pub fn map_ports(&mut self, ranges: impl IntoIterator<Item = RangeInclusive<u16>>) {
        let ranges: Vec<_> = ranges.into_iter().collect();
        let bus = self.context.bus.upgrade().expect("live I/O bus");
        validate_ranges(
            bus.connections
                .borrow()
                .iter()
                .flat_map(|device| &device.ports)
                .chain(&self.connection.ports)
                .chain(&ranges),
        );
        if !ranges.is_empty() {
            let read = PortRead::new(port_read(&self.connection.device));
            let write = PortWrite::new(port_write(&self.connection.device));
            for ports in &ranges {
                bus.ports
                    .set_read(self.context.reborrow(), ports.clone(), &read);
                bus.ports
                    .set_write(self.context.reborrow(), ports.clone(), &write);
            }
        }
        self.connection.ports.extend(ranges);
    }

    /// Maps physical ranges to this device's MMIO methods. The machine's physical
    /// map must also mark these addresses as [`relocade_x86::PhysicalMapping::Mmio`].
    pub fn map_mmio(&mut self, ranges: impl IntoIterator<Item = RangeInclusive<u32>>) {
        let ranges: Vec<_> = ranges.into_iter().collect();
        let bus = self.context.bus.upgrade().expect("live I/O bus");
        validate_ranges(
            bus.connections
                .borrow()
                .iter()
                .flat_map(|device| &device.mmio)
                .chain(&self.connection.mmio)
                .chain(&ranges),
        );
        self.connection.mmio.extend(ranges);
    }

    pub(super) fn finish(self) -> ConnectedDevice {
        self.connection
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
