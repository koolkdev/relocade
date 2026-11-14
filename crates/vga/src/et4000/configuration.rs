//! Firmware programming through the card's register operations.
use super::{registers::*, Et4000Ax};
use crate::{
    effects::{CardEffects, Effects},
    ports::VgaPortAccess,
    registers::crtc,
    PackedLayout,
};
use relocade_machine::IoContext;

impl Et4000Ax {
    pub fn ports_enabled(&self) -> bool {
        self.vga.setup.enabled()
    }

    /// Prepared indexed image with the currently programmed CRTC start address.
    pub fn packed_layout(&self) -> Option<PackedLayout> {
        self.output
            .display
            .packed_layout()
            .map(|layout| layout.with_start_offset(self.programmed_start() * 4))
    }

    /// Remove extended addressing, clocks and banks before common VGA font
    /// loading and mode programming. The firmware can then install new timings.
    pub fn prepare_legacy_mode(&mut self, mut context: IoContext<'_>) {
        if !self.ports_enabled() {
            return;
        }
        self.extensions.write_hercules(3);
        self.extensions.write_mode(0xa0);
        self.program_crtc(context.reborrow(), crtc::VERTICAL_RETRACE_END, 0);
        for (index, value) in [
            (0x32, 0),
            (EXTENDED_START, 0),
            (COMPATIBILITY, 8),
            (OVERFLOW, 0),
            (CONFIGURATION, 0x40),
            (0x37, 0x0b),
        ] {
            self.program_crtc(context.reborrow(), index, value);
        }
        for (index, value) in [(6, 0), (AUXILIARY, 0xbc)] {
            self.select_sequencer(index);
            self.write_sequencer_data(context.reborrow(), value);
        }
        self.with_effects(context.reborrow(), Effects::CONFIGURATION, |card| {
            card.extensions.write_attribute(ATTRIBUTE_MISC, 0);
        });
        self.write_segments(context, 0);
    }

    /// Firmware leaves the same index and accepted value as a guest CRTC write.
    pub fn program_crtc(&mut self, context: IoContext<'_>, index: u8, value: u8) {
        self.select_crtc(index);
        self.write_crtc_data(context, value);
    }

    pub fn clear_video_memory(&mut self, context: IoContext<'_>) {
        self.with_effects(context, Effects::IMAGE, |card| card.memory.clear_vram());
    }

    pub(super) fn write_segments(&mut self, context: IoContext<'_>, value: u8) {
        self.with_effects(context, Effects::MEMORY, |card| {
            card.extensions.write_segments(value);
        });
    }
}
