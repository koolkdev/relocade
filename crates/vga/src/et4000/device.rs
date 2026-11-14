//! One connection with concrete standard VGA port and memory operations.
use super::Et4000Ax;
use crate::{io::Io, mmio::MemoryMapping, ports::PortDecoding};
use relocade_machine::{Device, DeviceConnection};
use std::{cell::RefCell, rc::Rc};

impl Et4000Ax {
    pub fn configure_io(io: &mut DeviceConnection<'_>, device: &Rc<RefCell<Self>>) {
        assert!(device.borrow().io.is_none(), "ET4000 is already connected");
        let memory = MemoryMapping::new(io, device);
        let ports = Self::bind_ports(io, device);
        let mut card = device.borrow_mut();
        let decoding = card.memory_decoding();
        memory.update_decoding(io.context(), None, decoding);
        ports.update_decoding(io.context(), None, PortDecoding::vga(&card.vga));
        card.io = Some(Io { ports, memory });
    }
}

impl Device for Et4000Ax {
    async fn run(device: Rc<RefCell<Self>>) {
        crate::output::run(device).await;
    }
}
