//! One connection with concrete standard VGA port and memory operations.
use super::Et4000Ax;
use crate::{
    io::Io,
    memory::VideoMemory,
    mmio::{MemoryDecoding, MemoryMapping, VgaMemory},
    ports::PortDecoding,
    VgaCore,
};
use relocade_machine::{Device, DeviceConnection};
use std::{cell::RefCell, rc::Rc};

impl Et4000Ax {
    pub fn configure_io(io: &mut DeviceConnection<'_>, device: &Rc<RefCell<Self>>) {
        assert!(device.borrow().io.is_none(), "ET4000 is already connected");
        let memory = MemoryMapping::new(io, device, None);
        let ports = Self::bind_ports(io, device);
        let mut card = device.borrow_mut();
        memory.update_decoding(io.context(), None, MemoryDecoding::vga(&card.vga));
        ports.update_decoding(io.context(), None, PortDecoding::vga(&card.vga));
        card.io = Some(Io { ports, memory });
    }
}

impl Device for Et4000Ax {
    async fn run(device: Rc<RefCell<Self>>) {
        crate::output::run(device, |card| &mut card.output, Self::capture_if_due).await;
    }
}

impl VgaMemory for Et4000Ax {
    fn vga_memory(&mut self) -> (&mut VgaCore, &mut VideoMemory) {
        (&mut self.vga, &mut self.memory)
    }
}
