//! Independent CPU banks over one physical allocation. Prepared when controls change.
use super::{registers::CONFIGURATION, Et4000Ax, VIDEO_MEMORY_BYTES};
use crate::{
    memory::{VideoMemory, VramWindow},
    mmio::{MemoryDecoding, VgaMemory},
    VgaCore,
};

impl Et4000Ax {
    pub(super) fn memory_decoding(&self) -> MemoryDecoding {
        let MemoryDecoding::Vga {
            window,
            write_operation,
            mut read,
            mut write,
        } = MemoryDecoding::vga(&self.vga)
        else {
            return MemoryDecoding::Disabled;
        };
        // This AT board exposes the segmented aperture, not an additional
        // one-megabyte linear host aperture (CR36 bit 4).
        if self.extensions.crtc(CONFIGURATION).unwrap() & 0x10 != 0 {
            return MemoryDecoding::Disabled;
        }
        let read_bank = u32::from(self.extensions.segments >> 4);
        let write_bank = u32::from(self.extensions.segments & 15);
        if self.vga.registers.sequencer.chain_four() {
            // In the linear-byte system RPS/WPM and the planar ALU do not
            // participate. A segment names 64 KiB of physical bytes.
            let bytes = window.end - window.start;
            // Extended operation uses the 64-KiB aperture. The 128-KiB VGA
            // aperture ignores A16, as in the chip's LGI addressing table.
            let bank_window = |bank| VramWindow {
                cpu_start: window.start,
                vram_start: bank << 16,
                bytes: bytes.min(0x10000),
            };
            MemoryDecoding::Linear {
                read: bank_window(read_bank),
                write: bank_window(write_bank),
                window,
            }
        } else {
            // The plane-system segment extends the plane address above A15.
            // Only two bank bits reach the installed one-megabyte DRAM.
            read.vram_start = (read_bank << 18) & (VIDEO_MEMORY_BYTES - 1);
            write.vram_start = (write_bank << 18) & (VIDEO_MEMORY_BYTES - 1);
            MemoryDecoding::Vga {
                window,
                write_operation,
                read,
                write,
            }
        }
    }
}

impl VgaMemory for Et4000Ax {
    fn vga_memory(&mut self) -> (&mut VgaCore, &mut VideoMemory) {
        (&mut self.vga, &mut self.memory)
    }
}
