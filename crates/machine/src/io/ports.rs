//! Device-owned registration handles for the shared host and guest port map.

mod handlers;
mod map;

use super::{IoContext, State};
pub use handlers::{PortRead, PortWrite};
pub(crate) use map::Ports;
use std::{
    ops::RangeInclusive,
    rc::{Rc, Weak},
};

/// One port or an inclusive range of starting ports. Wide transfers remain one
/// callback even when they extend beyond the registered range.
pub struct PortRange(RangeInclusive<u16>);

impl From<u16> for PortRange {
    fn from(port: u16) -> Self {
        Self(port..=port)
    }
}

impl From<RangeInclusive<u16>> for PortRange {
    fn from(ports: RangeInclusive<u16>) -> Self {
        assert!(!ports.is_empty(), "empty port range");
        Self(ports)
    }
}

/// One device's port registrations, shared by clones. Keep this handle in the
/// device and update it from normal methods, including port and MMIO callbacks.
/// Obtain it with [`super::IoBus::ports`]; repeated calls for the same device
/// share its registrations, including any initial [`super::DeviceConnection`] ports.
/// Changes take effect immediately, before the next guest transfer. Dropping a
/// handle leaves entries installed; use remove_read/remove_write or clear.
/// The handle does not keep the bus alive. It may be used before CPU creation.
/// Supply the context received by the callback, or borrow one synchronously
/// with [`super::IoBus::with_context`] from stopped host code.
///
/// ```
/// use relocade_machine::{IoBus, PortRead, PortWrite};
/// # use relocade_machine::Device;
/// # use std::{cell::RefCell, rc::Rc};
/// # struct Registers;
/// # impl Device for Registers {}
/// let device = Rc::new(RefCell::new(Registers));
/// let io = IoBus::default();
/// let ports = io.ports(&device);
/// let status = PortRead::new(|_, _, _| 9);
/// io.with_context(|mut context| {
///     ports.set_read(context.reborrow(), 0x3da, &status);
///     ports.set_read(context.reborrow(), 0x80..=0x83, &status);
///     let index = PortWrite::new(|_context, _port, _bytes, _value| { /* index register */ });
///     ports.set_write(context.reborrow(), 0x3d4, &index);
///     ports.remove_read(context.reborrow(), 0x3da); // Reads now return FF bytes.
///     ports.clear(context);
/// });
/// ```
#[derive(Clone)]
pub struct DevicePorts {
    bus: Weak<State>,
    identity: Rc<()>,
}

impl DevicePorts {
    pub(in crate::io) fn new(bus: &Weak<State>) -> Self {
        Self {
            bus: bus.clone(),
            identity: Rc::new(()),
        }
    }

    /// Installs a reusable [`PortRead`] for one port or a range.
    /// Reads and writes have independent ownership. An edit must not overlap
    /// another device's registrations in the same direction. The whole range
    /// is checked before any entry changes. Wide transfers stay one callback.
    pub fn set_read(&self, context: IoContext<'_>, ports: impl Into<PortRange>, read: &PortRead) {
        self.map(&context)
            .ports
            .set_read(context, self, ports.into().0, Some(read));
    }

    /// Installs a reusable [`PortWrite`], with the same ownership
    /// rules as [`Self::set_read`]. Callback values have unused high bits cleared.
    pub fn set_write(
        &self,
        context: IoContext<'_>,
        ports: impl Into<PortRange>,
        write: &PortWrite,
    ) {
        self.map(&context)
            .ports
            .set_write(context, self, ports.into().0, Some(write));
    }

    /// Removes read registrations. Subsequent reads return FF bytes; this does
    /// not restore an older device handler or change the corresponding writes.
    pub fn remove_read(&self, context: IoContext<'_>, ports: impl Into<PortRange>) {
        self.map(&context)
            .ports
            .set_read(context, self, ports.into().0, None);
    }

    /// Removes write registrations. Subsequent writes are ignored.
    pub fn remove_write(&self, context: IoContext<'_>, ports: impl Into<PortRange>) {
        self.map(&context)
            .ports
            .set_write(context, self, ports.into().0, None);
    }

    /// Removes all this device's port entries, retaining the handle for reuse.
    pub fn clear(&self, context: IoContext<'_>) {
        self.map(&context).ports.clear(context, self);
    }

    fn map(&self, context: &IoContext<'_>) -> Rc<State> {
        assert!(
            Weak::ptr_eq(&self.bus, context.bus),
            "port registration belongs to another I/O bus"
        );
        self.bus
            .upgrade()
            .expect("device ports require their live bus")
    }
}
