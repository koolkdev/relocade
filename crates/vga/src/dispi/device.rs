//! Device connection and asynchronous activity.

use super::DispiAdapter;
use crate::{
    io::Io,
    mmio::{MmioWindow, VGA_REGIONS},
};
use relocade_machine::{Device, DeviceConnection};
use std::{cell::RefCell, rc::Rc};

impl DispiAdapter {
    /// Connects the VGA memory window and the VGA/DISPI ports.
    pub fn configure_io(io: &mut DeviceConnection<'_>, device: &Rc<RefCell<Self>>) {
        assert!(device.borrow().io.is_none(), "VGA is already connected");
        let memory_windows = vec![MmioWindow::bind(
            io,
            device,
            VGA_REGIONS,
            Self::vga_window_decoding,
        )];
        let ports = Self::bind_ports(io, device);
        device.borrow_mut().io = Some(Io {
            ports,
            memory_windows,
        });
    }
}

impl Device for DispiAdapter {
    async fn run(device: Rc<RefCell<Self>>) {
        crate::output::run(device).await;
    }
}
