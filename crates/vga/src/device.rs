//! Guest bus access and registration of the execution owner's output task.

use crate::{
    effects::Effects,
    memory::VramAccess,
    output,
    registers::{
        dispi, ADAPTER_CONTROL_PORT, DAC_DATA_PORT, DAC_MASK_PORT, MISC_OUTPUT_WRITE_PORT,
        OPTION_SELECT_PORT,
    },
    Vga,
};
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

    fn read_port(&mut self, _context: IoContext<'_>, port: u16, bytes: u8) -> u32 {
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
                        self.advance_raster(self.clock.now());
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

    fn write_port(&mut self, mut context: IoContext<'_>, port: u16, bytes: u8, value: u32) {
        if dispi::PORT_RANGE.contains(&port) {
            if !self.setup.enabled() {
                return;
            }
            let effects = self.dispi.write_effect(port, bytes);
            self.with_effects(context, effects, |vga| {
                if let dispi::WriteAction::ClearVram = vga.dispi.write_port(port, bytes, value) {
                    vga.memory.clear_vram();
                }
            });
            return;
        }
        // Decode each byte after preceding index writes have taken effect.
        for (index, byte) in value.to_le_bytes()[..usize::from(bytes)].iter().enumerate() {
            let port = port.wrapping_add(index as u16);
            match port {
                ADAPTER_CONTROL_PORT | OPTION_SELECT_PORT => {
                    self.with_effects(context.reborrow(), Effects::IO, |vga| {
                        if port == ADAPTER_CONTROL_PORT {
                            vga.setup.write_control(*byte);
                        } else {
                            vga.setup.write_option(*byte);
                        }
                    });
                }
                port if self.setup.enabled() => {
                    let effects = match port {
                        MISC_OUTPUT_WRITE_PORT => Effects::CONFIGURATION_AND_IO,
                        DAC_MASK_PORT | DAC_DATA_PORT => Effects::IMAGE,
                        _ => self.legacy_effect(self.registers.write_kind(port)),
                    };
                    self.with_effects(context.reborrow(), effects, |vga| {
                        vga.registers.write_port(port, *byte);
                    });
                }
                _ => {}
            }
        }
    }
}
