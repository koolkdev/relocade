//! Guest bus access and registration of the execution owner's output task.

use crate::{
    memory::VramAccess,
    output,
    registers::{dispi, WriteKind, ADAPTER_CONTROL_PORT, OPTION_SELECT_PORT},
    Vga,
};
use relocade_machine::Device;
use std::{cell::RefCell, rc::Rc};

impl Device for Vga {
    async fn run(device: Rc<RefCell<Self>>) {
        output::run(device).await;
    }

    fn read_mmio(&mut self, address: u32, bytes: u8) -> u64 {
        let access = VramAccess::new(&self.setup, &self.registers, &self.dispi);
        self.memory.read(address, bytes, access)
    }

    fn write_mmio(&mut self, address: u32, bytes: u8, value: u64) {
        self.capture_if_due();
        let access = VramAccess::new(&self.setup, &self.registers, &self.dispi);
        self.memory.write(address, bytes, value, access);
    }

    fn read_port(&mut self, port: u16, bytes: u8) -> u32 {
        if dispi::PORT_RANGE.contains(&port) {
            return if self.setup.enabled() {
                self.dispi.read_port(port, bytes)
            } else {
                u32::MAX >> ((4 - bytes) * 8)
            };
        }
        let mut value = [0; 4];
        for (index, byte) in value[..usize::from(bytes)].iter_mut().enumerate() {
            let port = port.wrapping_add(index as u16);
            *byte = match port {
                OPTION_SELECT_PORT => self.setup.read_option(),
                port if self.setup.enabled() => {
                    let status = if port == self.registers.status_port() {
                        self.advance_raster(self.clock.timestamp());
                        self.raster.input_status_1()
                    } else {
                        0
                    };
                    self.registers.read_port(port, status)
                }
                _ => 0xff,
            };
        }
        u32::from_le_bytes(value)
    }

    fn write_port(&mut self, port: u16, bytes: u8, value: u32) {
        if dispi::PORT_RANGE.contains(&port) {
            if !self.setup.enabled() {
                return;
            }
            self.prepare_write(self.dispi.write_kind(port, bytes));
            let previous_layout = self.dispi.display_layout();
            match self.dispi.write_port(port, bytes, value) {
                dispi::WriteEffect::None => {}
                dispi::WriteEffect::ClearVram => self.memory.clear_vram(),
            }
            if self.dispi.display_layout() != previous_layout {
                self.reconfigure(previous_layout, self.registers.status_port(), true);
            }
            return;
        }
        // Decode each byte after preceding index writes have taken effect.
        for (index, byte) in value.to_le_bytes()[..usize::from(bytes)].iter().enumerate() {
            let port = port.wrapping_add(index as u16);
            match port {
                ADAPTER_CONTROL_PORT | OPTION_SELECT_PORT => {
                    let was_enabled = self.setup.enabled();
                    if port == ADAPTER_CONTROL_PORT {
                        self.setup.write_control(*byte);
                    } else {
                        self.setup.write_option(*byte);
                    }
                    if self.setup.enabled() != was_enabled {
                        self.programming_changed.notify(usize::MAX);
                    }
                }
                port if self.setup.enabled() => {
                    let kind = self.registers.write_kind(port);
                    self.prepare_write(kind);
                    let previous_status_port = self.registers.status_port();
                    self.registers.write_port(port, *byte);
                    if kind == WriteKind::Timing {
                        self.reconfigure(self.dispi.display_layout(), previous_status_port, true);
                    }
                }
                _ => {}
            }
        }
    }
}

impl Vga {
    fn prepare_write(&mut self, kind: WriteKind) {
        match kind {
            WriteKind::Access => {}
            WriteKind::Display => self.capture_if_due(),
            WriteKind::Timing => self.advance_raster(self.clock.timestamp()),
        }
    }
}
