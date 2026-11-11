//! Individual port operations. Register banks own masks, indices and transfer
//! phases; these operations preserve display inputs before changing them.

use crate::{registers::WriteKind, Vga};
use relocade_machine::IoContext;

pub(super) fn read_open_bus(_: &mut Vga) -> u8 {
    0xff
}

pub(super) fn ignore_write(_: &mut Vga, _: IoContext<'_>, _: u8) {}

pub(super) fn read_option(vga: &mut Vga) -> u8 {
    vga.setup.read_option()
}

pub(super) fn write_option(vga: &mut Vga, context: IoContext<'_>, value: u8) {
    let was_enabled = vga.setup.enabled();
    vga.setup.write_option(value);
    vga.finish_setup_write(context, was_enabled);
}

pub(super) fn write_adapter_control(vga: &mut Vga, context: IoContext<'_>, value: u8) {
    let was_enabled = vga.setup.enabled();
    vga.setup.write_control(value);
    vga.finish_setup_write(context, was_enabled);
}

pub(super) fn read_attribute_address(vga: &mut Vga) -> u8 {
    vga.registers.attribute.address()
}

pub(super) fn read_attribute_data(vga: &mut Vga) -> u8 {
    vga.registers.attribute.read()
}

pub(super) fn write_attribute(vga: &mut Vga, _: IoContext<'_>, value: u8) {
    // The address phase also controls display enable.
    vga.capture_if_due();
    vga.registers.attribute.write(value);
}

pub(super) fn read_input_status_0(_: &mut Vga) -> u8 {
    0x10 // Attached color display; no retrace IRQ wired.
}

pub(super) fn read_misc_output(vga: &mut Vga) -> u8 {
    vga.registers.miscellaneous
}

pub(super) fn write_misc_output(vga: &mut Vga, context: IoContext<'_>, value: u8) {
    vga.advance_raster(vga.clock.now());
    let previous_status_port = vga.registers.status_port();
    vga.registers.miscellaneous = value;
    vga.reconfigure(context, vga.dispi.display_layout());
    if vga.registers.status_port() != previous_status_port {
        vga.programming_changed.notify(usize::MAX);
    }
}

pub(super) fn read_feature_control(vga: &mut Vga) -> u8 {
    vga.registers.feature_control
}

pub(super) fn read_crtc_index(vga: &mut Vga) -> u8 {
    vga.registers.crtc.index()
}

pub(super) fn write_crtc_index(vga: &mut Vga, _: IoContext<'_>, value: u8) {
    vga.registers.crtc.select(value);
}

pub(super) fn read_crtc_data(vga: &mut Vga) -> u8 {
    vga.registers.crtc.read()
}

pub(super) fn write_crtc_data(vga: &mut Vga, context: IoContext<'_>, value: u8) {
    let kind = vga.registers.crtc.write_kind();
    vga.prepare_write(kind);
    vga.registers.crtc.write(value);
    vga.finish_register_write(context, kind);
}

pub(super) fn read_sequencer_index(vga: &mut Vga) -> u8 {
    vga.registers.sequencer.index()
}

pub(super) fn write_sequencer_index(vga: &mut Vga, _: IoContext<'_>, value: u8) {
    vga.registers.sequencer.select(value);
}

pub(super) fn read_sequencer_data(vga: &mut Vga) -> u8 {
    vga.registers.sequencer.read()
}

pub(super) fn write_sequencer_data(vga: &mut Vga, context: IoContext<'_>, value: u8) {
    let kind = vga.registers.sequencer.write_kind();
    vga.prepare_write(kind);
    vga.registers.sequencer.write(value);
    vga.finish_register_write(context, kind);
}

pub(super) fn read_graphics_index(vga: &mut Vga) -> u8 {
    vga.registers.graphics.index()
}

pub(super) fn write_graphics_index(vga: &mut Vga, _: IoContext<'_>, value: u8) {
    vga.registers.graphics.select(value);
}

pub(super) fn read_graphics_data(vga: &mut Vga) -> u8 {
    vga.registers.graphics.read()
}

pub(super) fn write_graphics_data(vga: &mut Vga, _: IoContext<'_>, value: u8) {
    vga.prepare_write(vga.registers.graphics.write_kind());
    vga.registers.graphics.write(value);
}

pub(super) fn read_dac_mask(vga: &mut Vga) -> u8 {
    vga.registers.dac.mask
}

pub(super) fn write_dac_mask(vga: &mut Vga, _: IoContext<'_>, value: u8) {
    vga.capture_if_due();
    vga.registers.dac.mask = value;
}

pub(super) fn read_dac_state(vga: &mut Vga) -> u8 {
    vga.registers.dac.state()
}

pub(super) fn write_dac_read_address(vga: &mut Vga, _: IoContext<'_>, value: u8) {
    vga.registers.dac.select(value, true);
}

pub(super) fn read_dac_address(vga: &mut Vga) -> u8 {
    vga.registers.dac.address()
}

pub(super) fn write_dac_write_address(vga: &mut Vga, _: IoContext<'_>, value: u8) {
    vga.registers.dac.select(value, false);
}

pub(super) fn read_dac_data(vga: &mut Vga) -> u8 {
    vga.registers.dac.read()
}

pub(super) fn write_dac_data(vga: &mut Vga, _: IoContext<'_>, value: u8) {
    vga.capture_if_due();
    vga.registers.dac.write(value);
}

pub(super) fn read_dispi_index(vga: &mut Vga) -> u16 {
    vga.dispi.index()
}

pub(super) fn write_dispi_index(vga: &mut Vga, _: IoContext<'_>, value: u16) {
    vga.dispi.select(value);
}

pub(super) fn read_dispi_data(vga: &mut Vga) -> u16 {
    vga.dispi.read()
}

pub(super) fn write_dispi_data(vga: &mut Vga, context: IoContext<'_>, value: u16) {
    vga.prepare_write(vga.dispi.write_kind());
    let previous_layout = vga.dispi.display_layout();
    match vga.dispi.write(value) {
        crate::registers::dispi::WriteEffect::None => {}
        crate::registers::dispi::WriteEffect::ClearVram => vga.memory.clear_vram(),
    }
    if vga.dispi.display_layout() != previous_layout {
        vga.reconfigure(context, previous_layout);
    }
}

impl Vga {
    fn prepare_write(&mut self, kind: WriteKind) {
        match kind {
            WriteKind::Access => {}
            WriteKind::Display => self.capture_if_due(),
            WriteKind::Timing => self.advance_raster(self.clock.now()),
        }
    }

    fn finish_register_write(&mut self, context: IoContext<'_>, kind: WriteKind) {
        if kind == WriteKind::Timing {
            self.reconfigure(context, self.dispi.display_layout());
        }
    }

    fn finish_setup_write(&self, context: IoContext<'_>, was_enabled: bool) {
        if self.setup.enabled() != was_enabled {
            self.configure_ports(context);
            self.programming_changed.notify(usize::MAX);
        }
    }
}
