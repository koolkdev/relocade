//! Scoped device setup and adaptation of existing register methods.

use super::{
    validate_ranges, Device, IoContext, Port, PortBinding, PortPolicy, PortRange, PortRead,
    PortValue, PortWindow, PortWrite,
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
/// use relocade_machine::{Device, IoBus, PortPolicy};
/// use std::{cell::RefCell, rc::Rc};
/// struct Registers;
/// impl Device for Registers {}
/// let io = IoBus::default();
/// io.connect(Rc::new(RefCell::new(Registers)), |io| {
///     io.port(0x64, PortPolicy::BYTE, |_| 0x14, |_, _| {});
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

    /// Claims and configures one register using this connection's context.
    /// Pass the logical read/write functions directly; the policy determines
    /// their value type and how wider transfers behave. Use an FF read or no-op
    /// write for an unimplemented direction. The returned endpoint can prepare
    /// and select reusable handlers at later device state boundaries.
    pub fn port<T: PortValue>(
        &mut self,
        address: u16,
        policy: PortPolicy<T>,
        read: impl Fn(IoContext<'_>) -> T + 'static,
        write: impl Fn(IoContext<'_>, T) + 'static,
    ) -> Port<T> {
        let port = Port::new(self.claim_ports(address), policy, read, write);
        port.configure(self.context(), &port.read, &port.write);
        port
    }

    /// Claims a fixed window and its transfer policy. Supply its endpoints to
    /// components, then configure it from current device state. Newly claimed
    /// ports return open bus and ignore writes until configured.
    /// Also accepts a subrange wholly contained in one existing claim, including
    /// a range mapped with [`Self::map_ports`]. Partial overlaps and ranges
    /// spanning separate claims are rejected.
    pub fn ports<T: PortValue>(
        &mut self,
        ports: impl Into<PortRange>,
        policy: PortPolicy<T>,
    ) -> PortWindow<T> {
        PortWindow::new(self.claim_ports(ports), policy)
    }

    pub(in crate::io) fn claim_ports(&mut self, ports: impl Into<PortRange>) -> PortBinding {
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

    /// Maps ranges to this device's existing `read_port` and `write_port` methods.
    /// This compatibility adapter is for devices awaiting migration. New devices
    /// register individual ports with [`Self::port`] and an explicit policy
    /// or define windows for grouped configuration with [`Self::ports`].
    pub fn map_ports(&mut self, ranges: impl IntoIterator<Item = RangeInclusive<u16>>) {
        let ranges: Vec<_> = ranges.into_iter().collect();
        self.validate_ports(&ranges);
        if !ranges.is_empty() {
            let read = PortRead::for_device(&self.connection.device);
            let write = PortWrite::for_device(&self.connection.device);
            for ports in ranges {
                let port = self.claim_ports(ports);
                port.set_read(self.context(), &read);
                port.set_write(self.context(), &write);
            }
        }
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
