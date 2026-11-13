//! Transfer policies supplied by the device for CPU access to video memory.

/// A bounded CPU address window into contiguous physical VRAM. The device
/// supplies a window whose entire backing range is valid.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct VramWindow {
    pub cpu_start: u32,
    pub vram_start: u32,
    pub bytes: u32,
}

impl VramWindow {
    /// Translate an address already bounded by the card's MMIO bindings.
    pub(super) fn memory_offset(self, address: u32) -> u32 {
        self.vram_start + (address - self.cpu_start)
    }
}
