//! Concrete register writes and the effects declared by their register owners.

use super::Effects;
use crate::{registers::dispi::WriteAction, Vga};
use relocade_machine::IoContext;

impl Vga {
    pub(crate) fn write_sequencer_data(&mut self, context: IoContext<'_>, value: u8) {
        let effects = self.legacy_effect(self.registers.sequencer.write_effect());
        self.with_effects(context, effects, |vga| vga.registers.sequencer.write(value));
    }

    pub(crate) fn write_graphics_data(&mut self, context: IoContext<'_>, value: u8) {
        let effects = self.legacy_effect(self.registers.graphics.write_effect());
        self.with_effects(context, effects, |vga| vga.registers.graphics.write(value));
    }

    pub(crate) fn write_crtc_data(&mut self, context: IoContext<'_>, value: u8) {
        let effects = self.legacy_effect(self.registers.crtc.write_effect());
        self.with_effects(context, effects, |vga| vga.registers.crtc.write(value));
    }

    pub(crate) fn write_attribute(&mut self, context: IoContext<'_>, value: u8) {
        let effects = self.legacy_effect(self.registers.attribute.write_effect());
        self.with_effects(context, effects, |vga| vga.registers.attribute.write(value));
    }

    pub(crate) fn write_dac_mask(&mut self, context: IoContext<'_>, value: u8) {
        self.with_effects(context, Effects::IMAGE, |vga| {
            vga.registers.dac.mask = value
        });
    }

    pub(crate) fn write_dac_data(&mut self, context: IoContext<'_>, value: u8) {
        self.with_effects(context, Effects::IMAGE, |vga| {
            vga.registers.dac.write(value)
        });
    }

    pub(crate) fn write_misc_output(&mut self, context: IoContext<'_>, value: u8) {
        self.with_effects(context, Effects::CONFIGURATION_AND_IO, |vga| {
            vga.registers.miscellaneous = value;
        });
    }

    pub(crate) fn write_adapter_control(&mut self, context: IoContext<'_>, value: u8) {
        self.with_effects(context, Effects::IO, |vga| vga.setup.write_control(value));
    }

    pub(crate) fn write_option_select(&mut self, context: IoContext<'_>, value: u8) {
        self.with_effects(context, Effects::IO, |vga| vga.setup.write_option(value));
    }

    pub(crate) fn write_dispi_data(&mut self, context: IoContext<'_>, value: u16) {
        let effects = self.dispi.write_effect();
        self.with_effects(context, effects, |vga| {
            if let WriteAction::ClearVram = vga.dispi.write(value) {
                vga.memory.clear_vram();
            }
        });
    }
}
