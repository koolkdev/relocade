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
    /// Bytes beyond the CPU window do not spill into adjacent VRAM.
    pub(super) fn memory_offset(self, address: u32) -> Option<u32> {
        let offset = address.checked_sub(self.cpu_start)?;
        (offset < self.bytes).then(|| self.vram_start + offset)
    }
}
