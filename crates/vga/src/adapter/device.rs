//! Device connection and asynchronous activity.

use super::DispiAdapter;
use crate::mmio::MemoryMapping;
use crate::ports::Ports;
use relocade_machine::{Device, DeviceConnection};
use std::{cell::RefCell, rc::Rc};

/// Retained bindings and operations; ordinary accesses do not consult this state.
pub(super) struct Io {
    pub ports: Ports,
    pub memory: MemoryMapping,
}

impl DispiAdapter {
    /// Connects the memory aperture and individual VGA/DISPI port operations.
    pub fn configure_io(io: &mut DeviceConnection<'_>, device: &Rc<RefCell<Self>>) {
        assert!(device.borrow().io.is_none(), "VGA is already connected");
        let memory = Self::bind_memory(io, device);
        let ports = Self::bind_ports(io, device);
        let mut card = device.borrow_mut();
        memory.update_decoding(io.context(), None, card.memory_decoding());
        card.io = Some(Io { ports, memory });
    }
}

impl Device for DispiAdapter {
    async fn run(device: Rc<RefCell<Self>>) {
        crate::output::run(device, |card| &mut card.output, Self::capture_if_due).await;
    }
}
