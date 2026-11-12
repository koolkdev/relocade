//! INT 10h service selection. Implementations declare their own register ABI.

use super::{
    adapter, characters, control, cursor, firmware, fonts, graphics, information, modes, palette,
    scroll, state, strings, VideoFirmware,
};
use crate::screen;
use relocade_machine::{
    real_mode_interrupt::{interrupt, interrupt_dispatch},
    CpuContext,
};

#[interrupt(context(call), args(function = ah))]
pub(crate) async fn dispatch(call: &mut CpuContext<'_>, video: &VideoFirmware, function: u8) {
    interrupt_dispatch!(
        call,
        match function {
            0x00 => firmware::set_mode(video),
            0x01 => cursor::set_cursor_shape,
            0x02 => cursor::set_cursor,
            0x03 => cursor::cursor,
            0x05 => cursor::select_page,
            0x06 => scroll::scroll_up,
            0x07 => scroll::scroll_down,
            0x08 => characters::read_character,
            0x09 => characters::write_characters_with_attribute,
            0x0a => characters::write_characters_preserving_attributes,
            0x0b => legacy_color,
            0x0c => graphics::pixel::write_pixel,
            0x0d => graphics::pixel::read_pixel,
            0x0e => characters::teletype,
            0x0f => modes::mode,
            0x10 => palette_services,
            0x11 => font_services,
            0x12 => configuration,
            0x13 => string_output,
            0x1a => display_combination,
            0x1b => information::write_report,
            0x1c => state_services,
            0x4f => firmware::vbe(video),
            _ => crate::interrupts::unsupported,
        }
    );
}

#[interrupt(context(call), args(function = al))]
async fn string_output(call: &mut CpuContext<'_>, function: u8) {
    interrupt_dispatch!(
        call,
        match function {
            0x00 => strings::write_text,
            0x01 => strings::write_text_and_move_cursor,
            0x02 => strings::write_cells,
            0x03 => strings::write_cells_and_move_cursor,
            _ => crate::interrupts::unsupported,
        }
    );
}

#[interrupt(context(call), args(function = al))]
async fn state_services(call: &mut CpuContext<'_>, function: u8) {
    interrupt_dispatch!(
        call,
        match function {
            0x00 => state::required_blocks,
            0x01 => state::save,
            0x02 => state::restore,
            _ => state::unsupported,
        }
    );
}

#[interrupt(context(call), args(function = al))]
async fn palette_services(call: &mut CpuContext<'_>, function: u8) {
    interrupt_dispatch!(
        call,
        match function {
            0x00 => palette::attributes::set_register,
            0x01 => palette::attributes::set_overscan,
            0x02 => palette::attributes::load_palette,
            0x03 => palette::attributes::set_blink,
            0x07 => palette::attributes::register,
            0x08 => palette::attributes::overscan,
            0x09 => palette::attributes::read_palette,
            0x10 => palette::dac::set_color,
            0x12 => palette::dac::load_palette,
            0x13 => palette_paging,
            0x15 => palette::dac::color,
            0x17 => palette::dac::read_palette,
            0x18 => palette::dac::set_mask,
            0x19 => palette::dac::mask,
            0x1a => palette::paging::paging,
            0x1b => palette::dac::grayscale_range,
            _ => crate::interrupts::unsupported,
        }
    );
}

#[interrupt(context(call), args(function = bl))]
async fn palette_paging(call: &mut CpuContext<'_>, function: u8) {
    interrupt_dispatch!(
        call,
        match function {
            0x00 => palette::paging::set_mode,
            0x01 => palette::paging::select_page,
            _ => crate::interrupts::unsupported,
        }
    );
}

#[interrupt(context(call), args(function = al))]
async fn font_services(call: &mut CpuContext<'_>, function: u8) {
    interrupt_dispatch!(
        call,
        match function {
            0x00 => fonts::text::load_font,
            0x01 => fonts::text::load_8x14,
            0x02 => fonts::text::load_8x8,
            0x03 => fonts::text::select_maps,
            0x04 => fonts::text::load_8x16,
            0x10 => fonts::text::set_font,
            0x11 => fonts::text::set_8x14,
            0x12 => fonts::text::set_8x8,
            0x14 => fonts::text::set_8x16,
            0x20 => fonts::graphics::set_high_font,
            0x21 => fonts::graphics::set_font,
            0x22 => fonts::graphics::set_8x14,
            0x23 => fonts::graphics::set_8x8,
            0x24 => fonts::graphics::set_8x16,
            0x30 => fonts::information,
            _ => crate::interrupts::unsupported,
        }
    );
}

#[interrupt(context(call), args(function = bh))]
async fn legacy_color(call: &mut CpuContext<'_>, function: u8) {
    interrupt_dispatch!(
        call,
        match function {
            0x00 => palette::compatibility::set_background,
            0x01 => palette::compatibility::select_palette,
            _ => palette::compatibility::ignore_unknown_selector,
        }
    );
}

#[interrupt(context(call), args(function = al))]
async fn display_combination(call: &mut CpuContext<'_>, function: u8) {
    interrupt_dispatch!(
        call,
        match function {
            0x00 => adapter::display_codes,
            0x01 => adapter::set_display_codes,
            _ => crate::interrupts::unsupported,
        }
    );
}

#[interrupt(context(call), args(function = bl))]
async fn configuration(call: &mut CpuContext<'_>, function: u8) {
    interrupt_dispatch!(
        call,
        match function {
            0x10 => adapter::configuration,
            0x30 => modes::select_scanlines,
            0x31 => palette::load::set_palette_loading,
            0x32 => control::set_video_addressing,
            0x33 => palette::load::set_grayscale_summing,
            0x34 => screen::set_cursor_emulation,
            0x36 => control::set_screen_output,
            _ => crate::interrupts::unsupported,
        }
    );
}
