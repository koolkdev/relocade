//! Firmware viewports use bounded images even though native CRTC addresses wrap.
use super::{registers::EXTENDED_START, Et4000Ax};
use crate::{
    effects::{CardEffects, Effects},
    registers::{attribute, crtc},
    PackedLayout,
};
use relocade_machine::IoContext;

impl Et4000Ax {
    /// CRTC Offset counts eight bytes; the physical image must fit in VRAM.
    pub fn max_packed_pitch(&self) -> Option<u32> {
        Some(self.packed_layout()?.max_pitch().min(255 * 8) & !7)
    }

    /// Round up to the CRTC's eight-byte pitch and preserve the pixel origin.
    pub fn set_packed_pitch(&mut self, context: IoContext<'_>, pitch: u32) -> Option<PackedLayout> {
        let current = self.packed_layout()?;
        let pitch = pitch.checked_add(7)? & !7;
        if !self.ports_enabled() || pitch > self.max_packed_pitch()? {
            return None;
        }
        let next = PackedLayout::new(
            current.size(),
            current.format(),
            pitch,
            0,
            self.memory.capacity(),
        )?
        .with_origin(current.origin())?;
        self.set_viewport(context, next);
        self.packed_layout()
    }

    /// Program the coarse CRTC start and fine pixel pan together. The coarse
    /// counter still becomes visible to scanout at the next retrace latch.
    pub fn set_display_origin(&mut self, context: IoContext<'_>, origin: [u16; 2]) -> bool {
        if !self.ports_enabled() {
            return false;
        }
        let Some(next) = self.packed_layout().and_then(|current| {
            PackedLayout::new(
                current.size(),
                current.format(),
                current.pitch(),
                0,
                self.memory.capacity(),
            )?
            .with_origin(origin.map(u32::from))
        }) else {
            return false;
        };
        self.set_viewport(context, next);
        true
    }

    fn set_viewport(&mut self, context: IoContext<'_>, layout: PackedLayout) {
        self.with_effects(context, Effects::CONFIGURATION, |card| {
            let start = layout.display_start();
            let counter = start / 4;
            let crtc = &mut card.vga.registers.crtc.registers.values;
            crtc[usize::from(crtc::OFFSET)] = (layout.pitch() / 8) as u8;
            crtc[usize::from(crtc::START_ADDRESS_HIGH)] = (counter >> 8) as u8;
            crtc[usize::from(crtc::START_ADDRESS_LOW)] = counter as u8;
            crtc[usize::from(crtc::PRESET_ROW_SCAN)] &= !0x60;
            let high = card.extensions.crtc(EXTENDED_START).unwrap();
            card.extensions
                .write_crtc(EXTENDED_START, (high & !3) | (counter >> 16) as u8, false);
            card.vga.registers.attribute.values[usize::from(attribute::HORIZONTAL_PANNING)] =
                (start & 3) as u8;
        });
    }
}
