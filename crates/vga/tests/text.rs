mod common;
use common::{boundary, crtc, ManualClock};
use relocade_machine::Device;
use relocade_vga::{Vga, VideoFrame, TEXT_MEMORY_START};
use std::{rc::Rc, sync::Arc, time::Duration};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

fn indexed(vga: &mut impl Device, port: u16, index: u8, value: u8) {
    vga.write_port(port, 2, u32::from(u16::from_le_bytes([index, value])));
}

fn attribute(vga: &mut impl Device, index: u8, value: u8) {
    vga.read_port(0x3da, 1);
    vga.write_port(0x3c0, 1, u32::from(index));
    vga.write_port(0x3c0, 1, u32::from(value));
    vga.write_port(0x3c0, 1, 0x20);
}

fn setup() -> (Rc<ManualClock>, Vga<ManualClock>) {
    let clock = Rc::new(ManualClock::default());
    let mut vga = Vga::with_clock(clock.clone());
    for cell in 0..2000 {
        vga.write_mmio(TEXT_MEMORY_START + cell * 2, 2, 0x0720);
    }
    crtc(&mut vga, 0x0a, 0x20);
    (clock, vga)
}

fn cell(vga: &mut impl Device, index: u32, character: u8, attribute: u8) {
    vga.write_mmio(
        TEXT_MEMORY_START + index * 2,
        2,
        u64::from(u16::from_le_bytes([character, attribute])),
    );
}

fn font(vga: &mut impl Device, address: u16, bytes: &[u8]) {
    indexed(vga, 0x3c4, 2, 4);
    indexed(vga, 0x3c4, 4, 6);
    indexed(vga, 0x3ce, 5, 0);
    indexed(vga, 0x3ce, 6, 5);
    for (offset, &byte) in bytes.iter().enumerate() {
        vga.write_mmio(
            0xa0000 + u32::from(address) + offset as u32,
            1,
            u64::from(byte),
        );
    }
    indexed(vga, 0x3c4, 2, 3);
    indexed(vga, 0x3c4, 4, 2);
    indexed(vga, 0x3ce, 5, 0x10);
    indexed(vga, 0x3ce, 6, 0x0e);
}

fn capture(clock: &ManualClock, vga: &mut impl Device, at: Duration) {
    clock.set(at);
    vga.read_port(0x3da, 1);
}

fn pixel(frame: &VideoFrame, x: usize, y: usize) -> &[u8] {
    let offset = (y * frame.width as usize + x) * 4;
    &frame.pixels[offset..offset + 4]
}

fn first_frame(clock: &ManualClock, vga: &mut Vga<ManualClock>) -> Arc<VideoFrame> {
    capture(clock, vga, Duration::from_millis(50));
    vga.display().current_frame().unwrap()
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn bootstrap_font_orientation_colors_controls_and_last_cell() {
    let (clock, mut vga) = setup();
    for (index, character, color) in [
        (0, b'F', 0x6f),
        (1, 1, 7),
        (2, 0xc4, 7),
        (3, 0xb3, 7),
        (1999, 0xdb, 9),
    ] {
        cell(&mut vga, index, character, color);
    }
    let frame = first_frame(&clock, &mut vga);
    assert_eq!((frame.width, frame.height), (640, 400));
    for (row, bits) in [0xfe, 0x62, 0x68, 0x78, 0x68, 0x60, 0xf0, 0]
        .into_iter()
        .enumerate()
    {
        for x in 0..8 {
            let color = if bits & (0x80 >> x) != 0 {
                [255, 255, 255, 255]
            } else {
                [170, 85, 0, 255]
            };
            assert_eq!(pixel(&frame, x, row * 2), color);
            assert_eq!(pixel(&frame, x, row * 2 + 1), color);
        }
    }
    assert_eq!(pixel(&frame, 9, 0), [170, 170, 170, 255]);
    for x in 16..24 {
        assert_eq!(pixel(&frame, x, 8), [170, 170, 170, 255]);
    }
    for y in 0..16 {
        assert_eq!(pixel(&frame, 27, y), [170, 170, 170, 255]);
    }
    assert_eq!(pixel(&frame, 639, 399), [85, 85, 255, 255]);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn cursor_shape_skew_and_underline_use_programmed_scanlines() {
    for (address, start, end, visible) in [
        (82, 6, 7, true),
        (82, 0x26, 7, false),
        (82, 7, 6, false),
        (82, 8, 31, false),
        (2002, 6, 7, false),
    ] {
        let (clock, mut vga) = setup();
        crtc(&mut vga, 0x0e, (address >> 8) as u8);
        crtc(&mut vga, 0x0f, address as u8);
        crtc(&mut vga, 0x0a, start);
        crtc(&mut vga, 0x0b, end);
        let frame = first_frame(&clock, &mut vga);
        assert_eq!(pixel(&frame, 16, 27), [0, 0, 0, 255]);
        assert_eq!(
            pixel(&frame, 16, 28),
            if visible {
                [170, 170, 170, 255]
            } else {
                [0, 0, 0, 255]
            }
        );
    }
    let (clock, mut vga) = setup();
    crtc(&mut vga, 0x0a, 6);
    crtc(&mut vga, 0x0b, 0x27); // One-cell cursor delay.
    crtc(&mut vga, 0x0f, 79); // Delay into the border must not wrap to row one.
    cell(&mut vga, 0, b' ', 1);
    crtc(&mut vga, 0x14, 3);
    let frame = first_frame(&clock, &mut vga);
    assert_eq!(pixel(&frame, 0, 6), [0, 0, 170, 255]);
    assert_eq!(pixel(&frame, 0, 28), [0, 0, 0, 255]);
    crtc(&mut vga, 0x0f, 1);
    capture(&clock, &mut vga, boundary(5));
    let frame = vga.display().current_frame().unwrap();
    assert_eq!(pixel(&frame, 8, 12), [0, 0, 0, 255]);
    assert_eq!(pixel(&frame, 16, 12), [170, 170, 170, 255]);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn cursor_comparison_wraps_with_the_full_display_address_counter() {
    let (clock, mut vga) = setup();
    crtc(&mut vga, 0x0c, 0xff);
    crtc(&mut vga, 0x0d, 0xff);
    crtc(&mut vga, 0x0a, 6);
    crtc(&mut vga, 0x0b, 7);
    let frame = first_frame(&clock, &mut vga);
    assert_eq!(pixel(&frame, 0, 12), [0, 0, 0, 255]);
    assert_eq!(pixel(&frame, 8, 12), [170, 170, 170, 255]);
    crtc(&mut vga, 0x0b, 0x27);
    capture(&clock, &mut vga, boundary(5));
    let frame = vga.display().current_frame().unwrap();
    assert_eq!(pixel(&frame, 8, 12), [0, 0, 0, 255]);
    assert_eq!(pixel(&frame, 16, 12), [170, 170, 170, 255]);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn font_ram_character_height_and_row_stride_control_the_image() {
    let (clock, mut vga) = setup();
    let mut glyph = [0; 16];
    glyph[0] = 0x80;
    glyph[15] = 1;
    font(&mut vga, u16::from(b'A') * 32, &glyph);
    cell(&mut vga, 0, b'A', 7);
    cell(&mut vga, 4, b'A', 4); // Offset is four cells, despite a two-cell display.
    crtc(&mut vga, 0x11, 0x2e);
    crtc(&mut vga, 1, 1);
    crtc(&mut vga, 7, 0x1d);
    crtc(&mut vga, 0x12, 31);
    crtc(&mut vga, 9, 0x4f); // Sixteen rows, no double scanning.
    crtc(&mut vga, 0x13, 2);
    let frame = first_frame(&clock, &mut vga);
    assert_eq!((frame.width, frame.height), (16, 32));
    assert_eq!(pixel(&frame, 0, 0), [170, 170, 170, 255]);
    assert_eq!(pixel(&frame, 0, 1), [0, 0, 0, 255]);
    assert_eq!(pixel(&frame, 7, 15), [170, 170, 170, 255]);
    assert_eq!(pixel(&frame, 0, 16), [170, 0, 0, 255]);
    crtc(&mut vga, 8, 15); // Begin at the last scanline of the first character row.
    capture(&clock, &mut vga, boundary(5));
    let frame = vga.display().current_frame().unwrap();
    assert_eq!(pixel(&frame, 0, 0), [0, 0, 0, 255]);
    assert_eq!(pixel(&frame, 7, 0), [170, 170, 170, 255]);
    assert_eq!(pixel(&frame, 0, 1), [170, 0, 0, 255]);
    assert_eq!(pixel(&frame, 7, 16), [170, 0, 0, 255]);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn font_selection_and_ninth_dot_extension_are_independent_of_intensity() {
    let (clock, mut vga) = setup();
    font(&mut vga, 0xc4 * 32, &[0]);
    font(&mut vga, 0x4000 + 0xc4 * 32, &[1]);
    font(&mut vga, 0x4000 + u16::from(b'A') * 32, &[1]);
    indexed(&mut vga, 0x3c4, 3, 4); // A=bank1 at4000, B=bank0.
    indexed(&mut vga, 0x3c4, 1, 0); // Nine-dot cells.
    attribute(&mut vga, 0x13, 8); // Zero shift in nine-dot text.
    cell(&mut vga, 0, 0xc4, 7);
    cell(&mut vga, 1, 0xc4, 15);
    cell(&mut vga, 2, b'A', 15);
    cell(&mut vga, 3, b' ', 1);
    cell(&mut vga, 4, 0xc4, 1);
    crtc(&mut vga, 0x14, 3);
    let frame = first_frame(&clock, &mut vga);
    assert_eq!(frame.width, 720);
    assert_eq!(pixel(&frame, 7, 0), [0, 0, 0, 255]);
    assert_eq!(pixel(&frame, 16, 0), [170, 170, 170, 255]);
    assert_eq!(pixel(&frame, 17, 0), [170, 170, 170, 255]);
    assert_eq!(pixel(&frame, 26, 0), [0, 0, 0, 255]);
    // Ordinary underlines are dashed in nine-dot mode; line graphics extend.
    assert_eq!(pixel(&frame, 34, 6), [0, 0, 170, 255]);
    assert_eq!(pixel(&frame, 35, 6), [0, 0, 0, 255]);
    assert_eq!(pixel(&frame, 44, 6), [0, 0, 170, 255]);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn palettes_color_select_dac_mask_and_background_intensity_feed_pixel_output() {
    let (clock, mut vga) = setup();
    cell(&mut vga, 0, b' ', 0x97);
    attribute(&mut vga, 0x10, 4); // Bit7 is now background intensity, not blink.
    let frame = first_frame(&clock, &mut vga);
    assert_eq!(pixel(&frame, 0, 0), [85, 85, 255, 255]);
    cell(&mut vga, 0, 0xdb, 4);
    attribute(&mut vga, 4, 2);
    attribute(&mut vga, 0x14, 0x0b);
    attribute(&mut vga, 0x10, 0x84);
    vga.write_port(0x3c6, 1, 0x7f);
    vga.write_port(0x3c8, 1, 0x32);
    for value in [1, 2, 3] {
        vga.write_port(0x3c9, 1, value);
    }
    capture(&clock, &mut vga, boundary(5));
    let frame = vga.display().current_frame().unwrap();
    assert_eq!(pixel(&frame, 0, 0), [4, 8, 12, 255]);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn line_compare_restarts_addresses_and_can_clear_horizontal_panning() {
    let (clock, mut vga) = setup();
    for (index, color) in [(0, 1), (1, 3), (4, 4), (5, 14), (8, 2)] {
        cell(&mut vga, index, 0xdb, color);
    }
    crtc(&mut vga, 0x11, 0x2e);
    crtc(&mut vga, 1, 1);
    crtc(&mut vga, 7, 0x0d);
    crtc(&mut vga, 9, 0);
    crtc(&mut vga, 0x12, 3);
    crtc(&mut vga, 0x13, 2);
    crtc(&mut vga, 0x0d, 4);
    crtc(&mut vga, 0x18, 1);
    attribute(&mut vga, 0x13, 4);
    attribute(&mut vga, 0x10, 0x2c);
    let frame = first_frame(&clock, &mut vga);
    assert_eq!((frame.width, frame.height), (16, 4));
    assert_eq!(pixel(&frame, 0, 0), [170, 0, 0, 255]);
    assert_eq!(pixel(&frame, 4, 0), [255, 255, 85, 255]);
    assert_eq!(pixel(&frame, 0, 1), [0, 170, 0, 255]);
    assert_eq!(pixel(&frame, 4, 2), [0, 0, 170, 255]);
    assert_eq!(pixel(&frame, 0, 3), [170, 0, 0, 255]);
}
