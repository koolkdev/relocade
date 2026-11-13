//! Concrete register writes and the effects declared by their register owners.

use super::Effects;
use crate::{dispi::WriteAction, registers::sequencer, DispiAdapter};
use relocade_machine::IoContext;

impl DispiAdapter {
    pub(crate) fn write_sequencer_data(&mut self, context: IoContext<'_>, value: u8) {
        let mut effects = self.legacy_effect(self.vga.registers.sequencer.write_effect());
        effects.memory_decode = matches!(
            self.vga.registers.sequencer.index(),
            sequencer::MAP_MASK | sequencer::MEMORY_MODE
        );
        self.with_effects(context, effects, |card| card.vga.write_sequencer(value));
    }

    pub(crate) fn write_graphics_data(&mut self, context: IoContext<'_>, value: u8) {
        let mut effects = self.legacy_effect(self.vga.registers.graphics.write_effect());
        effects.memory_decode = true;
        self.with_effects(context, effects, |card| card.vga.write_graphics(value));
    }

    pub(crate) fn write_crtc_data(&mut self, context: IoContext<'_>, value: u8) {
        let effects = self.legacy_effect(self.vga.registers.crtc.write_effect());
        self.with_effects(context, effects, |card| {
            card.vga.registers.crtc.write(value)
        });
    }

    pub(crate) fn write_attribute(&mut self, context: IoContext<'_>, value: u8) {
        let effects = self.legacy_effect(self.vga.registers.attribute.write_effect());
        self.with_effects(context, effects, |card| {
            card.vga.registers.attribute.write(value)
        });
    }

    pub(crate) fn write_dac_mask(&mut self, context: IoContext<'_>, value: u8) {
        self.with_effects(context, Effects::IMAGE, |card| {
            card.vga.registers.dac.mask = value
        });
    }

    pub(crate) fn write_dac_data(&mut self, context: IoContext<'_>, value: u8) {
        self.with_effects(context, Effects::IMAGE, |card| {
            card.vga.registers.dac.write(value)
        });
    }

    pub(crate) fn write_misc_output(&mut self, context: IoContext<'_>, value: u8) {
        self.with_effects(context, Effects::CONFIGURATION_AND_IO, |card| {
            card.vga.registers.miscellaneous = value;
        });
    }

    pub(crate) fn write_adapter_control(&mut self, context: IoContext<'_>, value: u8) {
        self.with_effects(context, Effects::IO, |card| {
            card.vga.setup.write_control(value)
        });
    }

    pub(crate) fn write_option_select(&mut self, context: IoContext<'_>, value: u8) {
        self.with_effects(context, Effects::IO, |card| {
            card.vga.setup.write_option(value)
        });
    }

    pub(crate) fn write_dispi_data(&mut self, context: IoContext<'_>, value: u16) {
        self.write_dispi_register(context, self.dispi.index(), value);
    }

    pub(crate) fn write_dispi_register(&mut self, context: IoContext<'_>, index: u16, value: u16) {
        let effects = match index {
            crate::dispi::ENABLE => Effects::CONFIGURATION_AND_MEMORY,
            crate::dispi::BANK => Effects::MEMORY,
            crate::dispi::VIRTUAL_WIDTH | crate::dispi::X_OFFSET | crate::dispi::Y_OFFSET
                if self.dispi.display_layout().is_some() =>
            {
                Effects::CONFIGURATION
            }
            _ => Effects::NONE,
        };
        self.with_effects(context, effects, |card| {
            if let WriteAction::ClearVram =
                card.dispi
                    .write_register(index, value, card.memory.capacity())
            {
                card.memory.clear_vram();
            }
        });
    }
}
