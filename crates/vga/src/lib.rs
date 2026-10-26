//! VGA device state and scanout for the initial 80-by-25, 8-by-8 color text mode.
//!
//! The device owns video memory and CRTC start/cursor registers. Its text aperture
//! is MMIO, independent of system RAM. BIOS data-area bookkeeping belongs to the
//! BIOS; direct port writes change hardware state without changing that data.
//! Programmable graphics modes, font loading and the remaining VGA registers
//! are not implemented yet.
#![forbid(unsafe_code)]

mod crtc;
mod memory;
mod screen;
mod text_renderer;

use crtc::Crtc;
use memory::VideoMemory;
use relocade_machine::Device;
pub use screen::{TextCell, TextCursor, TextScreen};
use std::{ops::RangeInclusive, sync::Mutex};
pub use text_renderer::TextRenderer;

/// The color text aperture in the supported fixed mode.
pub const TEXT_MEMORY_START: u32 = 0xb8000;
pub const TEXT_MEMORY_BYTES: u32 = 0x8000;
pub const TEXT_MEMORY_RANGE: RangeInclusive<u32> =
    TEXT_MEMORY_START..=TEXT_MEMORY_START + TEXT_MEMORY_BYTES - 1;
pub const CRTC_INDEX_PORT: u16 = 0x3d4;
pub const CRTC_DATA_PORT: u16 = 0x3d5;
pub const CRTC_PORT_RANGE: RangeInclusive<u16> = CRTC_INDEX_PORT..=CRTC_DATA_PORT;

#[derive(Default)]
struct State {
    memory: VideoMemory,
    crtc: Crtc,
}

/// An emulated VGA device, shared with a display through `Arc<Vga>`.
/// Bus callbacks and snapshots take a short lock; rasterization runs afterward.
/// New video memory is zeroed. The BIOS initializes its text pages and registers.
///
/// Access can block. In a browser, execute device accesses and display snapshots
/// on workers, because the main thread cannot wait for a contended mutex. This
/// includes MMIO reached through a host memory view or BIOS initialization.
#[derive(Default)]
pub struct Vga {
    state: Mutex<State>,
}

impl Vga {
    pub fn new() -> Self {
        Self::default()
    }

    /// Copies display memory and hardware registers together, releasing the lock
    /// before returning. The snapshot stays owned and stable while guest writes
    /// continue or the CPU awaits a service. Browser callers must use a worker.
    /// A multi-operation BIOS update can still be observed between bus transfers.
    pub fn screen(&self) -> TextScreen {
        let state = self.state.lock().expect("VGA state");
        TextScreen {
            cells: std::array::from_fn(|index| {
                state
                    .memory
                    .cell(state.crtc.start_address.wrapping_add(index as u16))
            }),
            cursor: TextCursor {
                address: state.crtc.cursor_address,
                start_scan_line: state.crtc.cursor_start,
                end_scan_line: state.crtc.cursor_end,
            },
            start_address: state.crtc.start_address,
        }
    }
}

impl Device for Vga {
    fn read_mmio(&self, address: u32, bytes: u8) -> u64 {
        let state = self.state.lock().expect("VGA state");
        let mut value = [0; 8];
        for (index, byte) in value[..usize::from(bytes)].iter_mut().enumerate() {
            *byte = state
                .memory
                .read(address - TEXT_MEMORY_START + index as u32);
        }
        u64::from_le_bytes(value)
    }

    fn write_mmio(&self, address: u32, bytes: u8, value: u64) {
        let mut state = self.state.lock().expect("VGA state");
        for (index, byte) in value.to_le_bytes()[..usize::from(bytes)].iter().enumerate() {
            state
                .memory
                .write(address - TEXT_MEMORY_START + index as u32, *byte);
        }
    }

    fn read_port(&self, port: u16, bytes: u8) -> u32 {
        let state = self.state.lock().expect("VGA state");
        let mut value = [0; 4];
        for (index, byte) in value[..usize::from(bytes)].iter_mut().enumerate() {
            *byte = state.crtc.read(port.wrapping_add(index as u16));
        }
        u32::from_le_bytes(value)
    }

    fn write_port(&self, port: u16, bytes: u8, value: u32) {
        let mut state = self.state.lock().expect("VGA state");
        for (index, byte) in value.to_le_bytes()[..usize::from(bytes)].iter().enumerate() {
            state.crtc.write(port.wrapping_add(index as u16), *byte);
        }
    }
}
