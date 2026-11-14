//! Register decoding performed once when display programming changes.
use super::{VgaDisplayLayout, VgaGeometry};
use crate::{
    registers::{attribute, crtc, graphics, sequencer},
    FramebufferGeometry, Registers,
};

/// Decoder tags shared with the VGA shader.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub(crate) enum VgaMode {
    Text = 0,
    Planar = 1,
    Interleaved = 2,
    Indexed256 = 3,
    Unsupported = 4,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AddressPins {
    pub plane_mask: u32,
    pub shift: u32,
    pub wrap_shift: u32,
    pub wrap_mask: u32,
    pub row_scan_mask: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TextParameters {
    pub font_a: u32,
    pub font_b: u32,
    pub font_select_mask: u32,
    pub foreground_mask: u32,
    pub background_mask: u32,
    pub blink_mask: u32,
    pub line_graphics: bool,
    pub underline_row: u32,
    pub cursor_rows: [u32; 2],
    pub cursor_skew: u32,
}

/// Fixed inputs shared by native drawing and GPU decoding. Coordinates, VRAM,
/// DAC contents, display-start latches and blink phases are resolved at use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct VgaParameters {
    pub geometry: VgaGeometry,
    pub framebuffer: Option<FramebufferGeometry>,
    pub mode: VgaMode,
    pub blanked: bool,
    pub color_mask: u32,
    pub columns: u32,
    pub glyph_width: u32,
    pub row_height: u32,
    pub scan_repeat: u32,
    pub text_scanlines: u32,
    pub stride: u32,
    pub count_divisor: u32,
    pub split_scanline: u32,
    pub preset_row: u32,
    pub byte_pan: u32,
    pub pel_pan: u32,
    pub reset_panning: bool,
    pub load_interval: u32,
    pub pixels_per_clock: u32,
    pub pins: AddressPins,
    pub text: TextParameters,
    /// Attribute-to-DAC indices for each of the two live blink phases.
    pub palette: [[u8; 16]; 2],
    /// Half-open intervals already converted to output pixel coordinates.
    pub horizontal_blank: [[u32; 2]; 2],
    pub vertical_blank: [[u32; 2]; 2],
}

impl VgaParameters {
    pub fn new(registers: &Registers, layout: &VgaDisplayLayout) -> Self {
        let s = &registers.sequencer.values;
        let c = &registers.crtc.registers;
        let a = &registers.attribute.values;
        let attribute_mode = a[attribute::MODE_CONTROL as usize];
        let shift_mode = registers.graphics.values[graphics::MODE as usize];
        let text_mode = attribute_mode & 1 == 0;
        let mode = if text_mode {
            if registers.graphics.values[graphics::MISCELLANEOUS as usize] & 1 == 0 {
                VgaMode::Text
            } else {
                VgaMode::Unsupported
            }
        } else if (attribute_mode & 0x40 != 0) != (shift_mode & 0x40 != 0) {
            VgaMode::Unsupported
        } else if shift_mode & 0x40 != 0 {
            VgaMode::Indexed256
        } else if shift_mode & 0x20 != 0 {
            VgaMode::Interleaved
        } else {
            VgaMode::Planar
        };
        let geometry = VgaGeometry::prepare(registers, layout);
        let clocking = s[sequencer::CLOCKING_MODE as usize];
        let glyph_width = registers.sequencer.dots_per_character() as u32;
        let columns = c.horizontal_display() as u32;
        let maximum_row = c.values[crtc::MAXIMUM_SCAN_LINE as usize];
        let row_height = u32::from(maximum_row & 31) + 1;
        let scan_repeat = 1 << (maximum_row >> 7);
        let vertical_divisor = c.scanlines_per_vertical_count() as u32;
        let address_mode = c.values[crtc::MODE_CONTROL as usize];
        let underline = c.values[crtc::UNDERLINE_LOCATION as usize];
        let preset = c.values[crtc::PRESET_ROW_SCAN as usize];
        let pan = u32::from(a[attribute::HORIZONTAL_PANNING as usize]);
        let pel_pan = if mode == VgaMode::Indexed256 {
            (pan & 7) / 2
        } else if text_mode && glyph_width == 9 {
            if pan < 8 {
                pan + 1
            } else {
                0
            }
        } else {
            pan & 7
        };
        let (shift, wrap_shift, wrap_mask) = if layout.linear_addressing {
            (0, 0, 0)
        } else if underline & 0x40 != 0 {
            (2, 12, 3)
        } else if address_mode & 0x40 == 0 {
            (1, if address_mode & 0x20 != 0 { 15 } else { 13 }, 1)
        } else {
            (0, 0, 0)
        };
        let dots_per_pixel = if mode == VgaMode::Indexed256 { 2 } else { 1 };
        let skew = c.horizontal_display_skew() as u32;
        Self {
            geometry,
            framebuffer: match mode {
                VgaMode::Unsupported => None,
                VgaMode::Text => Some(FramebufferGeometry::Text {
                    columns: columns as u16,
                    rows: (layout.vertical_display * vertical_divisor / scan_repeat / row_height)
                        as u16,
                }),
                _ => Some(FramebufferGeometry::Graphics {
                    width: geometry.output.size[0] as u16,
                    height: geometry.output.size[1] as u16,
                }),
            },
            mode,
            blanked: clocking & 0x20 != 0
                || !registers.attribute.display_enabled()
                || mode == VgaMode::Unsupported,
            color_mask: if mode == VgaMode::Indexed256 {
                255
            } else {
                u32::from(a[attribute::COLOR_PLANE_ENABLE as usize] & 15)
            },
            columns,
            glyph_width,
            row_height,
            scan_repeat,
            text_scanlines: row_height * scan_repeat,
            stride: u32::from(c.values[crtc::OFFSET as usize]) * 2,
            count_divisor: if underline & 0x20 != 0 {
                4
            } else if address_mode & 8 != 0 {
                2
            } else {
                1
            },
            split_scanline: (layout.line_compare + 1) * vertical_divisor,
            preset_row: u32::from(preset & 31),
            byte_pan: u32::from((preset >> 5) & 3),
            pel_pan,
            reset_panning: attribute_mode & 0x20 != 0,
            load_interval: if clocking & 0x10 != 0 {
                4
            } else if clocking & 4 != 0 {
                2
            } else {
                1
            },
            pixels_per_clock: if mode == VgaMode::Indexed256 { 4 } else { 8 },
            pins: AddressPins {
                plane_mask: layout.memory_bytes / 4 - 1,
                shift,
                wrap_shift,
                wrap_mask,
                row_scan_mask: u32::from(!address_mode & 3) << 13,
            },
            text: TextParameters::new(registers),
            palette: std::array::from_fn(|phase| {
                std::array::from_fn(|color| {
                    let mut palette_index = color as u8 & a[attribute::COLOR_PLANE_ENABLE as usize];
                    if attribute_mode & 9 == 9 {
                        let blinking = attribute_mode & 2 != 0 || palette_index & 8 != 0;
                        palette_index =
                            (palette_index & 7) | if blinking && phase != 0 { 0 } else { 8 };
                    }
                    let select = a[attribute::COLOR_SELECT as usize];
                    let mut index = a[usize::from(palette_index)];
                    if attribute_mode & 0x80 != 0 {
                        index = (index & 15) | ((select & 3) << 4);
                    }
                    index | ((select & 12) << 4)
                })
            }),
            horizontal_blank: c.horizontal_blanking().map(|range| {
                [range.start, range.end]
                    .map(|edge| (edge.saturating_sub(skew) * glyph_width).div_ceil(dots_per_pixel))
            }),
            vertical_blank: layout.vertical_blanking.map(|range| {
                range.map(|edge| (edge * vertical_divisor).div_ceil(geometry.scanline_step))
            }),
        }
    }
}

impl TextParameters {
    fn new(registers: &Registers) -> Self {
        let maps = u32::from(registers.sequencer.values[sequencer::CHARACTER_MAP_SELECT as usize]);
        let extended = registers.sequencer.extended_memory();
        let mask = if extended { 0xffff } else { 0x3fff };
        let font_a = (((maps >> 2) & 3) * 0x4000 + ((maps >> 5) & 1) * 0x2000) & mask;
        let font_b = ((maps & 3) * 0x4000 + ((maps >> 4) & 1) * 0x2000) & mask;
        let dual_font = extended && font_a != font_b;
        let mode = registers.attribute.values[attribute::MODE_CONTROL as usize];
        let blinking = mode & 8 != 0;
        let c = &registers.crtc.registers.values;
        let cursor_start = c[crtc::CURSOR_START as usize];
        let cursor_end = c[crtc::CURSOR_END as usize];
        Self {
            font_a,
            font_b,
            font_select_mask: if dual_font { 8 } else { 0 },
            foreground_mask: if dual_font { 7 } else { 15 },
            background_mask: if blinking { 7 } else { 15 },
            blink_mask: if blinking { 0x80 } else { 0 },
            line_graphics: mode & 4 != 0,
            underline_row: u32::from(c[crtc::UNDERLINE_LOCATION as usize] & 31),
            cursor_rows: [
                u32::from(cursor_start & 31),
                if cursor_start & 0x20 != 0 {
                    0
                } else {
                    u32::from(cursor_end & 31) + 1
                },
            ],
            cursor_skew: u32::from((cursor_end >> 5) & 3),
        }
    }
}
