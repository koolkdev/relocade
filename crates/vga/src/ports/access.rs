//! Standard VGA operations used by concrete card port bindings.
use crate::{
    effects::{CardEffects, Effects},
    registers::{sequencer, WriteKind},
    VgaCore,
};
use relocade_machine::IoContext;

// This trait is used only by generic connection-time wiring. Every installed
// callback is specialized for its concrete card; guest accesses use no vtable.
pub(crate) trait VgaPortAccess: CardEffects + 'static {
    fn vga(&self) -> &VgaCore;
    fn vga_mut(&mut self) -> &mut VgaCore;

    fn read_input_status_0(&self) -> u8 {
        0x10
    }
    /// Card-specific status bits derived from the cached raster value.
    fn input_status_1(raster_status: u8) -> u8 {
        raster_status
    }
    fn read_adapter_control(&self) -> u8 {
        0xff
    }

    fn legacy_effect(&self, kind: WriteKind) -> Effects {
        Effects::vga(kind)
    }

    // Each bank retains one index. Cards can intercept these raw bytes before
    // applying VGA masks, and extend data access without replacing callbacks.
    fn select_sequencer(&mut self, value: u8) {
        self.vga_mut().registers.sequencer.select(value);
    }
    fn select_graphics(&mut self, value: u8) {
        self.vga_mut().registers.graphics.select(value);
    }
    fn select_crtc(&mut self, value: u8) {
        self.vga_mut().registers.crtc.select(value);
    }
    fn read_sequencer(&self) -> u8 {
        self.vga().registers.sequencer.read()
    }
    fn read_graphics(&self) -> u8 {
        self.vga().registers.graphics.read()
    }
    fn read_crtc(&self) -> u8 {
        self.vga().registers.crtc.read()
    }
    fn read_attribute(&self) -> u8 {
        self.vga().registers.attribute.read()
    }

    fn write_sequencer_data(&mut self, context: IoContext<'_>, value: u8) {
        let mut effects = self.legacy_effect(self.vga().registers.sequencer.write_effect());
        effects.memory_decode = matches!(
            self.vga().registers.sequencer.index(),
            sequencer::MAP_MASK | sequencer::MEMORY_MODE
        );
        self.with_effects(context, effects, |card| {
            card.vga_mut().write_sequencer(value)
        });
    }

    fn write_graphics_data(&mut self, context: IoContext<'_>, value: u8) {
        let mut effects = self.legacy_effect(self.vga().registers.graphics.write_effect());
        effects.memory_decode = true;
        self.with_effects(context, effects, |card| {
            card.vga_mut().write_graphics(value)
        });
    }

    fn write_crtc_data(&mut self, context: IoContext<'_>, value: u8) {
        let effects = self.legacy_effect(self.vga().registers.crtc.write_effect());
        self.with_effects(context, effects, |card| {
            card.vga_mut().registers.crtc.write(value)
        });
    }

    fn write_attribute(&mut self, context: IoContext<'_>, value: u8) {
        let effects = self.legacy_effect(self.vga().registers.attribute.write_effect());
        self.with_effects(context, effects, |card| {
            card.vga_mut().registers.attribute.write(value)
        });
    }

    fn write_dac_mask(&mut self, context: IoContext<'_>, value: u8) {
        self.with_effects(context, Effects::IMAGE, |card| {
            card.vga_mut().registers.dac.mask = value
        });
    }

    fn write_dac_data(&mut self, context: IoContext<'_>, value: u8) {
        self.with_effects(context, Effects::IMAGE, |card| {
            card.vga_mut().registers.dac.write(value)
        });
    }

    fn write_misc_output(&mut self, context: IoContext<'_>, value: u8) {
        self.with_effects(context, Effects::CONFIGURATION_AND_IO, |card| {
            card.vga_mut().registers.miscellaneous = value;
        });
    }

    fn write_adapter_control(&mut self, context: IoContext<'_>, value: u8) {
        self.with_effects(context, Effects::IO, |card| {
            card.vga_mut().setup.write_control(value)
        });
    }

    fn write_option_select(&mut self, context: IoContext<'_>, value: u8) {
        self.with_effects(context, Effects::IO, |card| {
            card.vga_mut().setup.write_option(value)
        });
    }
}
