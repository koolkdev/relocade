//! Firmware viewport operations over DISPI or the shared VGA registers.
use super::DispiAdapter;
use crate::{
    dispi,
    effects::{CardEffects, Effects},
    PackedLayout, Viewport,
};
use relocade_machine::IoContext;

impl DispiAdapter {
    /// The physical packed image used by scanout.
    pub fn packed_layout(&self) -> Option<PackedLayout> {
        self.dispi.display_layout()
    }

    /// The programmed logical image, including its firmware addressing limits.
    pub fn viewport(&self) -> Option<Viewport> {
        if !self.ports_enabled() {
            return None;
        }
        if let Some(layout) = self.packed_layout() {
            return Some(Viewport::packed(
                layout,
                self.memory.capacity(),
                layout.format().bytes_per_pixel(),
                u32::from(u16::MAX) * layout.format().bytes_per_pixel(),
            ));
        }
        let registers = &self.vga.registers;
        self.output.display.crtc_viewport(
            registers,
            u32::from(registers.crtc.programmed_start()),
            registers.graphics.memory_window().len() as u32,
        )
    }

    /// Round the byte pitch to the active mode's units and preserve its origin.
    pub fn set_display_pitch(&mut self, context: IoContext<'_>, pitch: u32) -> Option<Viewport> {
        let next = self.viewport()?.with_pitch(pitch)?;
        if self.packed_layout().is_some() {
            self.program_dispi_register(context, dispi::VIRTUAL_WIDTH, next.virtual_width() as u16);
        } else {
            self.with_effects(context, Effects::CONFIGURATION, |card| {
                next.write_crtc(&mut card.vga.registers);
            });
        }
        Some(next)
    }

    /// Move a bounded viewport, preserving overdue output before programming it.
    pub fn set_display_origin(&mut self, mut context: IoContext<'_>, origin: [u16; 2]) -> bool {
        let Some(next) = self
            .viewport()
            .and_then(|layout| layout.with_origin(origin.map(u32::from)))
        else {
            return false;
        };
        if self.packed_layout().is_some() {
            self.program_dispi_register(context.reborrow(), dispi::X_OFFSET, origin[0]);
            self.program_dispi_register(context, dispi::Y_OFFSET, origin[1]);
        } else {
            self.with_effects(context, Effects::CONFIGURATION, |card| {
                next.write_crtc(&mut card.vga.registers);
            });
        }
        true
    }
}
