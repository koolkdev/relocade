//! CPU memory windows and VGA address decoding.
mod window;
pub(crate) use window::MmioWindow;

use crate::{
    core::MemoryWrite,
    memory::{VideoMemory, VramWindow, VGA_MEMORY_BYTES},
    VgaCore,
};
use std::ops::{Range, RangeInclusive};

// The VGA memory-map selector enables different combinations of these regions.
pub(crate) const VGA_REGIONS: [RangeInclusive<u32>; 3] =
    [0xa0000..=0xaffff, 0xb0000..=0xb7fff, 0xb8000..=0xbffff];

#[derive(Clone, PartialEq, Eq)]
pub(crate) enum MemoryDecoding {
    Disabled,
    Vga {
        window: Range<u32>,
        write_operation: MemoryWrite,
        read: VramWindow,
        write: VramWindow,
    },
    Linear {
        window: Range<u32>,
        read: VramWindow,
        write: VramWindow,
    },
}

/// Borrow the independent core and backing together for a concrete memory callback.
pub(crate) trait VgaMemory {
    fn vga_memory(&mut self) -> (&mut VgaCore, &mut VideoMemory);
}

impl MemoryDecoding {
    pub fn linear(window: VramWindow) -> Self {
        Self::Linear {
            window: window.cpu_start..window.cpu_start + window.bytes,
            read: window,
            write: window,
        }
    }

    pub fn vga(vga: &VgaCore) -> Self {
        if !vga.decodes_video_memory() {
            Self::Disabled
        } else {
            let window = vga.memory_window();
            let backing = VramWindow {
                cpu_start: window.start,
                vram_start: 0,
                bytes: VGA_MEMORY_BYTES,
            };
            Self::Vga {
                window,
                write_operation: vga.memory_write(),
                read: backing,
                write: backing,
            }
        }
    }
}
