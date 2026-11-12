//! Fixed device port bindings backed by the shared host and guest port map.

mod callbacks;
mod handlers;
mod map;
mod widths;

use super::{IoContext, State};
pub use callbacks::DeviceCallbacks;
pub use handlers::{PortRead, PortWrite};
pub(crate) use map::Ports;
use std::{
    ops::RangeInclusive,
    rc::{Rc, Weak},
};

/// One port or an inclusive range of starting addresses. The installed operation
/// determines transfer behavior; byte sequences carry their own physical window.
pub struct PortRange(pub(super) RangeInclusive<u16>);

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

/// A fixed port or range claimed during [`super::DeviceConnection`].
/// Retain this binding to select handlers at device state changes. Changes take
/// effect before the next transfer, including the next element of a REP string
/// instruction. Reads and writes can be configured independently.
///
/// A binding cannot claim more ports or release its claim. Dropping it leaves
/// the current handlers installed. It does not keep the bus alive.
/// Use the callback's context, or [`super::IoBus::with_context`] from stopped
/// host code. The same configuration method can run during connection and later:
///
/// ```
/// use relocade_machine::{Device, IoBus, IoContext, PortBinding, PortRead};
/// use std::{cell::RefCell, rc::Rc};
/// struct Registers { status: Option<PortBinding>, ready: bool, idle: PortRead, busy: PortRead }
/// impl Device for Registers {}
/// impl Registers {
///     fn configure(&self, context: IoContext<'_>) {
///         let read = if self.ready { &self.idle } else { &self.busy };
///         self.status.as_ref().unwrap().set_read(context, read);
///     }
/// }
/// let device = Rc::new(RefCell::new(Registers {
///     status: None, ready: true,
///     idle: PortRead::new(|_, _, _| 0), busy: PortRead::new(|_, _, _| 1),
/// }));
/// let io = IoBus::new(device.clone(), |io| {
///     device.borrow_mut().status = Some(io.port(0x64));
///     device.borrow().configure(io.context());
/// });
/// assert_eq!(io.read_port(0x64, 1), 0);
/// device.borrow_mut().ready = false;
/// io.with_context(|context| device.borrow().configure(context));
/// assert_eq!(io.read_port(0x64, 1), 1);
/// ```
#[derive(Clone)]
pub struct PortBinding {
    bus: Weak<State>,
    ports: RangeInclusive<u16>,
}

impl PortBinding {
    pub(super) fn new(bus: &Weak<State>, ports: RangeInclusive<u16>) -> Self {
        Self {
            bus: bus.clone(),
            ports,
        }
    }

    /// Selects a reusable read operation for this binding's fixed ports.
    pub fn set_read(&self, context: IoContext<'_>, read: &PortRead) {
        self.bus(&context)
            .ports
            .set_read(context, self.ports.clone(), read);
    }

    /// Selects a reusable write operation without changing reads.
    pub fn set_write(&self, context: IoContext<'_>, write: &PortWrite) {
        self.bus(&context)
            .ports
            .set_write(context, self.ports.clone(), write);
    }

    fn bus(&self, context: &IoContext<'_>) -> Rc<State> {
        assert!(
            Weak::ptr_eq(&self.bus, context.bus),
            "port binding belongs to another I/O bus"
        );
        self.bus
            .upgrade()
            .expect("port binding requires its live bus")
    }
}
