//! One connection with concrete standard VGA port and memory operations.
use super::Et4000Ax;
use crate::{
    io::Io,
    mmio::{MmioWindow, VGA_REGIONS},
    ports::PortDecoding,
};
use relocade_machine::{Device, DeviceConnection};
use std::{cell::RefCell, rc::Rc};

impl Et4000Ax {
    pub fn configure_io(io: &mut DeviceConnection<'_>, device: &Rc<RefCell<Self>>) {
        assert!(device.borrow().io.is_none(), "ET4000 is already connected");
        let memory_windows = vec![MmioWindow::bind(
            io,
            device,
            VGA_REGIONS,
            Self::memory_decoding,
        )];
        let ports = Self::bind_ports(io, device);
        let mut card = device.borrow_mut();
        ports.update_decoding(io.context(), None, PortDecoding::vga(&card.vga));
        card.io = Some(Io {
            ports,
            memory_windows,
        });
    }
}

impl Device for Et4000Ax {
    async fn run(device: Rc<RefCell<Self>>) {
        crate::output::run(device).await;
    }
}
