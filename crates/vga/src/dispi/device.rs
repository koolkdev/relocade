//! Device connection and asynchronous activity.

use super::{DispiAdapter, LINEAR_FRAMEBUFFER_RANGE};
use crate::{
    io::Io,
    mmio::{MmioWindow, VGA_REGIONS},
};
use relocade_machine::{Device, DeviceConnection};
use std::{cell::RefCell, rc::Rc};

impl DispiAdapter {
    /// Connects the VGA window, the linear framebuffer and the VGA/DISPI ports.
    pub fn configure_io(io: &mut DeviceConnection<'_>, device: &Rc<RefCell<Self>>) {
        assert!(device.borrow().io.is_none(), "VGA is already connected");
        let memory_windows = vec![
            MmioWindow::bind(io, device, VGA_REGIONS, Self::vga_window_decoding),
            MmioWindow::bind(
                io,
                device,
                [LINEAR_FRAMEBUFFER_RANGE],
                Self::linear_framebuffer_decoding,
            ),
        ];
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
