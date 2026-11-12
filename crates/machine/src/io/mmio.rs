//! Fixed memory windows and prepared operations shared by host and guest access.

mod handlers;
mod map;

use super::{IoContext, State};
pub use handlers::{MmioRead, MmioWrite};
pub(crate) use map::Mmio;
use relocade_x86::MmioHandlerId;
use std::rc::Weak;

/// A fixed physical address range claimed by [`super::DeviceConnection::bind_mmio`].
/// The bus retains its operations; retaining this handle is only necessary to
/// replace reads or writes later. Replacement is immediate, including from an
/// operation in progress. This handle does not keep the bus alive.
///
/// Complete pages dispatch directly to the installed operation. Partial pages
/// retain range resolution, splitting transfers at device boundaries and holes.
#[derive(Clone)]
pub struct MmioBinding {
    bus: Weak<State>,
    handler: MmioHandlerId,
}

impl MmioBinding {
    pub(super) fn new(bus: &Weak<State>, handler: MmioHandlerId) -> Self {
        Self {
            bus: bus.clone(),
            handler,
        }
    }

    pub fn set_read(&self, context: IoContext<'_>, read: &MmioRead) {
        self.check_bus(&context);
        self.bus
            .upgrade()
            .expect("live MMIO bus")
            .mmio
            .set_read(context, self.handler, read);
    }

    pub fn set_write(&self, context: IoContext<'_>, write: &MmioWrite) {
        self.check_bus(&context);
        self.bus
            .upgrade()
            .expect("live MMIO bus")
            .mmio
            .set_write(context, self.handler, write);
    }

    fn check_bus(&self, context: &IoContext<'_>) {
        assert!(
            Weak::ptr_eq(&self.bus, context.bus),
            "MMIO binding belongs to another bus"
        );
    }
}
