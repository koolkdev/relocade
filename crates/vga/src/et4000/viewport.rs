//! Firmware viewports use bounded images even though native CRTC addresses wrap.
use super::{
    registers::{CONFIGURATION, EXTENDED_START},
    Et4000Ax,
};
use crate::{
    effects::{CardEffects, Effects},
    Viewport,
};
use relocade_machine::IoContext;

impl Et4000Ax {
    /// The full programmed image, including the card's addressing and pitch limits.
    pub fn viewport(&self) -> Option<Viewport> {
        if !self.ports_enabled() {
            return None;
        }
        if let Some(layout) = self.packed_layout() {
            return Some(Viewport::packed(layout, self.memory.capacity(), 8, 255 * 8));
        }
        if self.extensions.crtc(CONFIGURATION).unwrap() & 0x10 != 0 {
            return None; // This board does not connect the relocated linear CPU aperture.
        }
        self.output.display.crtc_viewport(
            &self.vga.registers,
            self.programmed_start(),
            self.memory.capacity(),
        )
    }

    /// Round to the CRTC pitch unit while preserving the pixel origin.
    pub fn set_display_pitch(&mut self, context: IoContext<'_>, pitch: u32) -> Option<Viewport> {
        let next = self.viewport()?.with_pitch(pitch)?;
        self.set_viewport(context, next);
        Some(next)
    }

    /// Program a bounded origin. Its coarse counter is latched at the next retrace.
    pub fn set_display_origin(&mut self, context: IoContext<'_>, origin: [u16; 2]) -> bool {
        let Some(next) = self
            .viewport()
            .and_then(|layout| layout.with_origin(origin.map(u32::from)))
        else {
            return false;
        };
        self.set_viewport(context, next);
        true
    }

    fn set_viewport(&mut self, context: IoContext<'_>, layout: Viewport) {
        self.with_effects(context, Effects::CONFIGURATION, |card| {
            let start = layout.write_crtc(&mut card.vga.registers);
            let high = card.extensions.crtc(EXTENDED_START).unwrap();
            card.extensions
                .write_crtc(EXTENDED_START, (high & !3) | (start >> 16) as u8, false);
        });
    }
}
