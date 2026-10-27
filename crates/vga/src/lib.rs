//! VGA device state and scanout for the initial 80-by-25, 8-by-8 color text mode.
//!
//! The device owns video memory and CRTC start/cursor registers. Its text aperture
//! is MMIO, independent of system RAM. BIOS data-area bookkeeping belongs to the
//! BIOS; direct port writes change hardware state without changing that data.
//! Programmable graphics modes, font loading and the remaining VGA registers
//! are not implemented yet.
#![forbid(unsafe_code)]

mod crtc;
mod display;
mod memory;
mod screen;
mod text_renderer;

use crtc::Crtc;
pub use display::Display;
use memory::VideoMemory;
use relocade_machine::Device;
pub use screen::{TextCell, TextCursor, TextScreen};
use std::ops::RangeInclusive;
pub use text_renderer::TextRenderer;

/// The color text aperture in the supported fixed mode.
pub const TEXT_MEMORY_START: u32 = 0xb8000;
pub const TEXT_MEMORY_BYTES: u32 = 0x8000;
pub const TEXT_MEMORY_RANGE: RangeInclusive<u32> =
    TEXT_MEMORY_START..=TEXT_MEMORY_START + TEXT_MEMORY_BYTES - 1;
pub const CRTC_INDEX_PORT: u16 = 0x3d4;
pub const CRTC_DATA_PORT: u16 = 0x3d5;
pub const CRTC_PORT_RANGE: RangeInclusive<u16> = CRTC_INDEX_PORT..=CRTC_DATA_PORT;

/// An emulated VGA device owned by the execution worker.
/// Registers and video memory are ordinary mutable state. Other workers receive
/// completed screens through [`Display`], without accessing the live device.
/// New video memory is zeroed. The BIOS initializes its text pages and registers.
#[derive(Default)]
pub struct Vga {
    memory: VideoMemory,
    crtc: Crtc,
    display: Display,
    dirty: bool,
}

impl Vga {
    pub fn new() -> Self {
        Self::default()
    }

    /// Retains the latest published screen independently of device ownership.
    pub fn display(&self) -> Display {
        self.display.clone()
    }

    /// Publishes changes as one immutable screen. Call at the display cadence,
    /// before awaiting input and before reporting completion. This is an explicit
    /// text-display boundary, not an emulation of VGA scanout or retrace timing.
    pub fn publish(&mut self) {
        if self.dirty {
            self.display.publish(self.screen());
            self.dirty = false;
        }
    }

    /// Copies the current device state on its owning worker. The returned screen
    /// stays stable across later device writes; it does not publish to displays.
    pub fn screen(&self) -> TextScreen {
        TextScreen {
            cells: std::array::from_fn(|index| {
                self.memory
                    .cell(self.crtc.start_address.wrapping_add(index as u16))
            }),
            cursor: TextCursor {
                address: self.crtc.cursor_address,
                start_scan_line: self.crtc.cursor_start,
                end_scan_line: self.crtc.cursor_end,
            },
            start_address: self.crtc.start_address,
        }
    }
}

impl Device for Vga {
    fn read_mmio(&mut self, address: u32, bytes: u8) -> u64 {
        let mut value = [0; 8];
        for (index, byte) in value[..usize::from(bytes)].iter_mut().enumerate() {
            *byte = self.memory.read(address - TEXT_MEMORY_START + index as u32);
        }
        u64::from_le_bytes(value)
    }

    fn write_mmio(&mut self, address: u32, bytes: u8, value: u64) {
        self.dirty = true;
        for (index, byte) in value.to_le_bytes()[..usize::from(bytes)].iter().enumerate() {
            self.memory
                .write(address - TEXT_MEMORY_START + index as u32, *byte);
        }
    }

    fn read_port(&mut self, port: u16, bytes: u8) -> u32 {
        let mut value = [0; 4];
        for (index, byte) in value[..usize::from(bytes)].iter_mut().enumerate() {
            *byte = self.crtc.read(port.wrapping_add(index as u16));
        }
        u32::from_le_bytes(value)
    }

    fn write_port(&mut self, port: u16, bytes: u8, value: u32) {
        self.dirty = true;
        for (index, byte) in value.to_le_bytes()[..usize::from(bytes)].iter().enumerate() {
            self.crtc.write(port.wrapping_add(index as u16), *byte);
        }
    }
}

impl Drop for Vga {
    fn drop(&mut self) {
        // Cancellation may drop the owner without reaching its next publication.
        self.publish();
    }
}
