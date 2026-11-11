//! Fixed device port bindings backed by the shared host and guest port map.

mod callbacks;
mod handlers;
mod indexed;
mod map;
mod policy;
mod registers;
mod widths;
mod windows;

use super::{IoContext, State};
pub use callbacks::DeviceCallbacks;
pub use handlers::{PortRead, PortWrite};
pub use indexed::IndexedPorts;
pub(crate) use map::Ports;
pub use policy::{PortPolicy, PortValue};
pub use registers::{BytePort, Port, WordPort};
use std::{
    ops::RangeInclusive,
    rc::{Rc, Weak},
};
pub use windows::PortWindow;

/// One port or an inclusive address window. The selected transfer policy
/// determines how wider accesses address the registers inside it.
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

/// Internal fixed binding shared by window configuration and the legacy adapter.
/// Handler replacement publishes directly to host routing and guest tables.
#[derive(Clone)]
pub(crate) struct PortBinding {
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

#[cfg(test)]
mod tests;
