//! Concrete register writes and the effects declared by their register owners.

use crate::effects::{CardEffects, Effects};
use crate::{dispi::registers::WriteAction, DispiAdapter};
use relocade_machine::IoContext;

impl DispiAdapter {
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
            if let WriteAction::Enable {
                clear_memory,
                dac_bits_per_component,
            } = card
                .dispi
                .write_register(index, value, card.memory.capacity())
            {
                if clear_memory {
                    card.memory.clear_vram();
                }
                card.vga
                    .registers
                    .dac
                    .set_bits_per_component(dac_bits_per_component);
            }
        });
    }
}
