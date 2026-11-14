//! Device connection and asynchronous activity.

use super::DispiAdapter;
use crate::{io::Io, mmio::MemoryMapping};
use relocade_machine::{Device, DeviceConnection};
use std::{cell::RefCell, rc::Rc};

impl DispiAdapter {
    /// Connects the memory aperture and individual VGA/DISPI port operations.
    pub fn configure_io(io: &mut DeviceConnection<'_>, device: &Rc<RefCell<Self>>) {
        assert!(device.borrow().io.is_none(), "VGA is already connected");
        let memory = MemoryMapping::new(io, device);
        let ports = Self::bind_ports(io, device);
        let mut card = device.borrow_mut();
        memory.update_decoding(io.context(), None, card.memory_decoding());
        card.io = Some(Io { ports, memory });
    }
}

impl Device for DispiAdapter {
    async fn run(device: Rc<RefCell<Self>>) {
        crate::output::run(device).await;
    }
}
