//! Index/data wiring without owning the component's register selection state.

use super::{Port, PortRead, PortValue, PortWrite};
use crate::IoContext;

/// A register selector and its data endpoint within one fixed window. The device
/// owns the selected register; this pair adds no access-time dispatch or state.
pub struct IndexedPorts<T> {
    pub(super) index: Port<T>,
    pub data: Port<T>,
}

impl<T: PortValue> IndexedPorts<T> {
    /// Defines index readback and selection. Publish changed data handling in
    /// `select` before returning, including between bytes of a wide transfer.
    pub fn index(
        mut self,
        read: impl Fn(IoContext<'_>) -> T + 'static,
        select: impl Fn(IoContext<'_>, T) + 'static,
    ) -> Self {
        self.index = self.index.define(read, select);
        self
    }

    /// Publishes index operations and the final data pair in one configuration.
    /// No intermediate data definition is installed before the chosen handlers.
    pub fn configure(&self, mut context: IoContext<'_>, read: &PortRead, write: &PortWrite) {
        self.index
            .configure(context.reborrow(), &self.index.read, &self.index.write);
        self.data.configure(context, read, write);
    }
}
