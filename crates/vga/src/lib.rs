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
use std::{
    ops::RangeInclusive,
    sync::{Mutex, MutexGuard, TryLockError},
};
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
#[derive(Default)]
pub struct Vga {
    state: Mutex<State>,
}

impl Vga {
    pub fn new() -> Self {
        Self::default()
    }

    /// Samples display memory and hardware registers together without waiting.
    /// Returns `None` when another access holds the device; keep the previous
    /// frame and try again on the next redraw. A successful snapshot stays owned
    /// and stable while guest writes continue or the CPU awaits a service.
    /// A multi-operation BIOS update can still be observed between bus transfers.
    pub fn screen(&self) -> Option<TextScreen> {
        let state = self.try_lock()?;
        Some(TextScreen {
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
        })
    }

    fn try_lock(&self) -> Option<MutexGuard<'_, State>> {
        match self.state.try_lock() {
            Ok(state) => Some(state),
            Err(TryLockError::WouldBlock) => None,
            Err(TryLockError::Poisoned(_)) => panic!("VGA state poisoned"),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.state.lock().expect("VGA state")
        }
        // The browser main thread cannot block in a Wasm atomic wait. Guarded
        // work is bounded Rust-only memory/register access: it never awaits or
        // calls host code, so releasing the lock cannot require a browser event.
        #[cfg(target_arch = "wasm32")]
        loop {
            if let Some(state) = self.try_lock() {
                return state;
            }
            std::hint::spin_loop();
        }
    }
}

impl Device for Vga {
    fn read_mmio(&self, address: u32, bytes: u8) -> u64 {
        let state = self.lock();
        let mut value = [0; 8];
        for (index, byte) in value[..usize::from(bytes)].iter_mut().enumerate() {
            *byte = state
                .memory
                .read(address - TEXT_MEMORY_START + index as u32);
        }
        u64::from_le_bytes(value)
    }

    fn write_mmio(&self, address: u32, bytes: u8, value: u64) {
        let mut state = self.lock();
        for (index, byte) in value.to_le_bytes()[..usize::from(bytes)].iter().enumerate() {
            state
                .memory
                .write(address - TEXT_MEMORY_START + index as u32, *byte);
        }
    }

    fn read_port(&self, port: u16, bytes: u8) -> u32 {
        let state = self.lock();
        let mut value = [0; 4];
        for (index, byte) in value[..usize::from(bytes)].iter_mut().enumerate() {
            *byte = state.crtc.read(port.wrapping_add(index as u16));
        }
        u32::from_le_bytes(value)
    }

    fn write_port(&self, port: u16, bytes: u8, value: u32) {
        let mut state = self.lock();
        for (index, byte) in value.to_le_bytes()[..usize::from(bytes)].iter().enumerate() {
            state.crtc.write(port.wrapping_add(index as u16), *byte);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Vga;

    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn busy_device_defers_the_display_snapshot() {
        let vga = Vga::new();
        let state = vga.lock();
        assert!(vga.screen().is_none());
        drop(state);
        assert!(vga.screen().is_some());
    }
}
