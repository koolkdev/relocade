//! Guest bus access and registration of the execution owner's output task.

use crate::{
    memory::VramAccess,
    output,
    registers::{dispi::Dispi, setup::Setup, Registers},
    Vga,
};
use relocade_machine::{Device, IoContext};
use std::{cell::RefCell, rc::Rc};

impl Device for Vga {
    async fn run(device: Rc<RefCell<Self>>) {
        output::run(device).await;
    }

    fn read_mmio(&mut self, _context: IoContext<'_>, address: u32, bytes: u8) -> u64 {
        let access = select_memory_access(&self.setup, &self.registers, &self.dispi);
        self.memory.read(address, bytes, access)
    }

    fn write_mmio(&mut self, _context: IoContext<'_>, address: u32, bytes: u8, value: u64) {
        self.capture_if_due();
        let access = select_memory_access(&self.setup, &self.registers, &self.dispi);
        self.memory.write(address, bytes, value, access);
    }
}

fn select_memory_access<'a>(
    setup: &Setup,
    registers: &'a Registers,
    dispi: &Dispi,
) -> VramAccess<'a> {
    if !setup.enabled() || !registers.video_memory_enabled() {
        VramAccess::Disabled
    } else if let Some(window) = dispi.bank_mapping() {
        VramAccess::Window(window)
    } else {
        VramAccess::Vga(registers)
    }
}
