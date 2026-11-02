//! INT 10h service selection. Implementations declare their own register ABI.

use super::{
    adapter, characters, cursor, fonts, graphics, information, modes, palette, scroll, state,
    strings,
};
use crate::screen;
use relocade_machine::real_mode_interrupt::interrupt_dispatch;

interrupt_dispatch! {
    pub(crate) dispatch(ah) {
        0x00: modes::set_mode,
        0x01: cursor::set_cursor_shape,
        0x02: cursor::set_cursor,
        0x03: cursor::cursor,
        0x05: cursor::select_page,
        0x06: scroll::scroll_up,
        0x07: scroll::scroll_down,
        0x08: characters::read_character,
        0x09: characters::write_characters_with_attribute,
        0x0a: characters::write_characters_preserving_attributes,
        0x0b: legacy_color,
        0x0c: graphics::pixel::write_pixel,
        0x0d: graphics::pixel::read_pixel,
        0x0e: characters::teletype,
        0x0f: modes::mode,
        0x10: palette_services,
        0x11: font_services,
        0x12: configuration,
        0x13: strings::interrupt,
        0x1a: display_combination,
        0x1b: information::interrupt,
        0x1c: state::interrupt,
        _: crate::interrupt::unsupported,
    }
}

interrupt_dispatch! {
    palette_services(al) {
        0x00: palette::attributes::set_register,
        0x01: palette::attributes::set_overscan,
        0x02: palette::attributes::load_palette,
        0x03: palette::attributes::set_blink,
        0x07: palette::attributes::register,
        0x08: palette::attributes::overscan,
        0x09: palette::attributes::read_palette,
        0x10: palette::dac::set_color,
        0x12: palette::dac::load_palette,
        0x13: palette_paging,
        0x15: palette::dac::color,
        0x17: palette::dac::read_palette,
        0x18: palette::dac::set_mask,
        0x19: palette::dac::mask,
        0x1a: palette::paging::paging,
        0x1b: palette::dac::grayscale_range,
        _: crate::interrupt::unsupported,
    }
}

interrupt_dispatch! {
    palette_paging(bl) {
        0x00: palette::paging::set_mode,
        0x01: palette::paging::select_page,
        _: crate::interrupt::unsupported,
    }
}

interrupt_dispatch! {
    font_services(al) {
        0x00: fonts::text::load_font,
        0x01: fonts::text::load_8x14,
        0x02: fonts::text::load_8x8,
        0x03: fonts::text::select_maps,
        0x04: fonts::text::load_8x16,
        0x10: fonts::text::set_font,
        0x11: fonts::text::set_8x14,
        0x12: fonts::text::set_8x8,
        0x14: fonts::text::set_8x16,
        0x20: fonts::graphics::set_high_font,
        0x21: fonts::graphics::set_font,
        0x22: fonts::graphics::set_8x14,
        0x23: fonts::graphics::set_8x8,
        0x24: fonts::graphics::set_8x16,
        0x30: fonts::information,
        _: crate::interrupt::unsupported,
    }
}

interrupt_dispatch! {
    legacy_color(bh) {
        0x00: palette::compatibility::set_background,
        0x01: palette::compatibility::select_palette,
        _: palette::compatibility::ignore_unknown_selector,
    }
}

interrupt_dispatch! {
    display_combination(al) {
        0x00: adapter::display_codes,
        0x01: adapter::set_display_codes,
        _: crate::interrupt::unsupported,
    }
}

interrupt_dispatch! {
    configuration(bl) {
        0x10: adapter::configuration,
        0x30: modes::select_scanlines,
        0x31: palette::load::set_palette_loading,
        0x33: palette::load::set_grayscale_summing,
        0x34: screen::set_cursor_emulation,
        0x36: modes::set_screen_output,
        _: crate::interrupt::unsupported,
    }
}
