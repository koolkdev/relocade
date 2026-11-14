//! Card programming shared by firmware and guest register accesses.

use crate::{dispi, ports::VgaPortAccess, DispiAdapter, DispiMode};
use relocade_machine::IoContext;

impl DispiAdapter {
    /// Whether adapter setup permits CPU port access, independently of scanout.
    pub fn ports_enabled(&self) -> bool {
        self.vga.setup.enabled()
    }

    pub fn video_memory_bytes(&self) -> u32 {
        self.memory.capacity()
    }

    pub fn video_bank(&self) -> u16 {
        self.dispi.bank()
    }

    /// Leave packed access before standard VGA font loading and mode programming.
    pub fn prepare_legacy_mode(&mut self, context: IoContext<'_>) {
        if self.ports_enabled() {
            self.program_dispi_register(context, dispi::ENABLE, dispi::DISABLED);
        }
    }

    /// Enter a fresh packed mode, resetting its bank and viewport. Re-entering the
    /// same mode also clears VRAM unless preservation was requested.
    pub fn enter_packed_mode(
        &mut self,
        mut context: IoContext<'_>,
        mode: DispiMode,
        preserve_memory: bool,
    ) -> bool {
        if !self.ports_enabled() || mode.layout(self.memory.capacity()).is_none() {
            return false;
        }
        let [width, height] = mode.size().map(|value| value as u16);
        let flags = dispi::ENABLED
            | if preserve_memory {
                dispi::NO_CLEAR_MEMORY
            } else {
                0
            };
        for (index, value) in [
            (dispi::ENABLE, dispi::DISABLED),
            (dispi::X_RESOLUTION, width),
            (dispi::Y_RESOLUTION, height),
            (dispi::BITS_PER_PIXEL, u16::from(mode.bits_per_pixel())),
            (dispi::ENABLE, flags),
        ] {
            self.program_dispi_register(context.reborrow(), index, value);
        }
        // BIOS mode entry enables CPU VRAM access while retaining color selection.
        self.write_misc_output(context, self.vga.registers.miscellaneous | 2);
        true
    }

    /// Select a backed 64-KiB CPU window without affecting the displayed image.
    pub fn set_video_bank(&mut self, context: IoContext<'_>, bank: u16) -> bool {
        if !self.ports_enabled()
            || self.packed_layout().is_none()
            || u32::from(bank) >= self.memory.capacity() / dispi::BANK_BYTES
        {
            return false;
        }
        self.program_dispi_register(context, dispi::BANK, bank);
        true
    }

    pub(super) fn program_dispi_register(
        &mut self,
        context: IoContext<'_>,
        index: u16,
        value: u16,
    ) {
        // Firmware leaves the guest-visible index on its last write.
        self.dispi.select(index);
        self.write_dispi_register(context, index, value);
    }
}
