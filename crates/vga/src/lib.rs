//! VGA device state and scanout for the initial 80-by-25, 8-by-8 color text mode.
//!
//! The device owns video memory and VGA register state. Its text aperture
//! is MMIO, independent of system RAM. BIOS data-area bookkeeping belongs to the
//! BIOS; direct port writes change hardware state without changing that data.
//! Programmable graphics modes, font loading and the remaining VGA registers
//! are not implemented yet.
#![forbid(unsafe_code)]

mod memory;
pub mod registers;
mod screen;
mod text_renderer;

use memory::VideoMemory;
use registers::Registers;
pub use registers::{CRTC_DATA_PORT, CRTC_INDEX_PORT, CRTC_PORT_RANGE, PORT_RANGES};
use relocade_machine::Device;
pub use screen::{TextCell, TextCursor, TextScreen};
use std::ops::RangeInclusive;
pub use text_renderer::TextRenderer;

/// The color text aperture in the supported fixed mode.
pub const TEXT_MEMORY_START: u32 = 0xb8000;
pub const TEXT_MEMORY_BYTES: u32 = 0x8000;
pub const TEXT_MEMORY_RANGE: RangeInclusive<u32> =
    TEXT_MEMORY_START..=TEXT_MEMORY_START + TEXT_MEMORY_BYTES - 1;

/// An emulated VGA device owned by the execution worker.
/// Registers and video memory are ordinary mutable state. Copy an owned
/// [`TextScreen`] before passing observations to another worker.
/// New video memory is zeroed. The BIOS initializes its text pages and registers.
#[derive(Default)]
pub struct Vga {
    memory: VideoMemory,
    registers: Registers,
}

impl Vga {
    pub fn new() -> Self {
        Self::default()
    }

    /// Copies the current device state on its owning worker. The returned screen
    /// stays stable across later device writes.
    pub fn screen(&self) -> TextScreen {
        let crtc = &self.registers.crtc;
        TextScreen {
            cells: std::array::from_fn(|index| {
                self.memory
                    .cell(crtc.start_address().wrapping_add(index as u16))
            }),
            cursor: TextCursor {
                address: crtc.cursor_address(),
                start_scan_line: crtc.cursor_start(),
                end_scan_line: crtc.cursor_end(),
            },
            start_address: crtc.start_address(),
        }
    }
}

impl Device for Vga {
    fn read_mmio(&mut self, address: u32, bytes: u8) -> u64 {
        let mut value = [0; 8];
        if !self.registers.video_memory_enabled() {
            value[..usize::from(bytes)].fill(0xff);
            return u64::from_le_bytes(value);
        }
        for (index, byte) in value[..usize::from(bytes)].iter_mut().enumerate() {
            *byte = self.memory.read(address - TEXT_MEMORY_START + index as u32);
        }
        u64::from_le_bytes(value)
    }

    fn write_mmio(&mut self, address: u32, bytes: u8, value: u64) {
        if !self.registers.video_memory_enabled() {
            return;
        }
        for (index, byte) in value.to_le_bytes()[..usize::from(bytes)].iter().enumerate() {
            self.memory
                .write(address - TEXT_MEMORY_START + index as u32, *byte);
        }
    }

    fn read_port(&mut self, port: u16, bytes: u8) -> u32 {
        let mut value = [0; 4];
        for (index, byte) in value[..usize::from(bytes)].iter_mut().enumerate() {
            *byte = self.registers.read_port(port.wrapping_add(index as u16));
        }
        u32::from_le_bytes(value)
    }

    fn write_port(&mut self, port: u16, bytes: u8, value: u32) {
        for (index, byte) in value.to_le_bytes()[..usize::from(bytes)].iter().enumerate() {
            self.registers
                .write_port(port.wrapping_add(index as u16), *byte);
        }
    }
}
