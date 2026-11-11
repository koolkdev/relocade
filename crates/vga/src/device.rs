//! MMIO access and the execution owner's output activity.

use crate::{memory::VramAccess, output, Vga};
use relocade_machine::{Device, IoContext};
use std::{cell::RefCell, rc::Rc};

impl Device for Vga {
    async fn run(device: Rc<RefCell<Self>>) {
        output::run(device).await;
    }

    fn read_mmio(&mut self, _context: IoContext<'_>, address: u32, bytes: u8) -> u64 {
        let access = VramAccess::new(&self.setup, &self.registers, &self.dispi);
        self.memory.read(address, bytes, access)
    }

    fn write_mmio(&mut self, _context: IoContext<'_>, address: u32, bytes: u8, value: u64) {
        self.capture_if_due();
        let access = VramAccess::new(&self.setup, &self.registers, &self.dispi);
        self.memory.write(address, bytes, value, access);
    }
}
