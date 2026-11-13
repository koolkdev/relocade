//! Scoped device setup and adaptation of existing register methods.

use super::{
    validate_ranges, Device, IoContext, MmioBinding, MmioRead, MmioWrite, PortBinding, PortRange,
    PortRead, PortWrite,
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
/// use relocade_machine::{Device, IoBus, PortRead, PortWrite};
/// use std::{cell::RefCell, rc::Rc};
/// struct Registers;
/// impl Device for Registers {}
/// let io = IoBus::default();
/// io.connect(Rc::new(RefCell::new(Registers)), |io| {
///     io.bind(0x64, &PortRead::byte(|_| 0x14), &PortWrite::byte(|_, _| {}));
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

    /// Borrows the same context used by port and MMIO callbacks. Use it to run
    /// the device's ordinary configuration method during initial connection.
    pub fn context(&mut self) -> IoContext<'_> {
        self.context.reborrow()
    }

    /// Claims one fixed port or range for this connection. Newly claimed ports
    /// initially return open bus and ignore writes until handlers are selected.
    /// Also accepts a subrange wholly contained in one of this connection's
    /// existing claims.
    /// Partial overlaps and ranges spanning separate claims are rejected.
    /// The returned binding may be retained; ownership lasts for the bus's lifetime.
    pub fn port(&mut self, ports: impl Into<PortRange>) -> PortBinding {
        let ports = ports.into().0;
        if !self
            .connection
            .ports
            .iter()
            .any(|claim| claim.contains(ports.start()) && claim.contains(ports.end()))
        {
            self.validate_ports(std::slice::from_ref(&ports));
            self.connection.ports.push(ports.clone());
        }
        PortBinding::new(self.context.bus, ports)
    }

    /// Claims fixed addresses and installs both operations immediately. Dropping
    /// the binding leaves the operations installed; retain it only to replace
    /// reads or writes later. Subranges of an existing claim are also accepted.
    pub fn bind(
        &mut self,
        ports: impl Into<PortRange>,
        read: &PortRead,
        write: &PortWrite,
    ) -> PortBinding {
        let binding = self.port(ports);
        binding.set_read(self.context(), read);
        binding.set_write(self.context(), write);
        binding
    }

    fn validate_ports(&self, ranges: &[RangeInclusive<u16>]) {
        let bus = self.context.bus.upgrade().expect("live I/O bus");
        validate_ranges(
            bus.connections
                .borrow()
                .iter()
                .flat_map(|device| &device.ports)
                .chain(&self.connection.ports)
                .chain(ranges),
        );
    }

    /// Claims a fixed physical window and installs its prepared operations.
    /// The physical map must mark these addresses as [`relocade_x86::PhysicalMapping::MMIO`].
    /// Transfers split at window boundaries; complete pages call the installed operations directly.
    /// Retain the binding only to replace reads or writes later.
    pub fn bind_mmio(
        &mut self,
        addresses: RangeInclusive<u32>,
        read: &MmioRead,
        write: &MmioWrite,
    ) -> MmioBinding {
        self.validate_mmio(std::slice::from_ref(&addresses));
        self.connection.mmio.push(addresses.clone());
        let bus = self.context.bus.upgrade().expect("live I/O bus");
        let handler = bus.mmio.bind(self.context(), addresses, read, write);
        MmioBinding::new(&bus.identity, handler)
    }

    fn validate_mmio(&self, ranges: &[RangeInclusive<u32>]) {
        let bus = self.context.bus.upgrade().expect("live I/O bus");
        validate_ranges(
            bus.connections
                .borrow()
                .iter()
                .flat_map(|device| &device.mmio)
                .chain(&self.connection.mmio)
                .chain(ranges),
        );
    }

    pub(super) fn finish(self) -> ConnectedDevice {
        self.connection
    }
}
