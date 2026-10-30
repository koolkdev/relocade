use super::reference::render;
use crate::common::fixture::{set_cell, text_device};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

fn pixel(pixels: &[u8], x: usize, y: usize) -> &[u8] {
    let start = (y * 640 + x) * 4;
    &pixels[start..start + 4]
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn glyph_orientation_colors_and_last_cell() {
    let mut vga = text_device();
    set_cell(&mut vga, 0, b'F', 0x6f);
    set_cell(&mut vga, 1999, 0xdb, 0x09);
    let pixels = render(&vga.snapshot());
    assert_eq!(pixels.len(), 640 * 200 * 4);
    for (y, bits) in [0xfe, 0x62, 0x68, 0x78, 0x68, 0x60, 0xf0, 0]
        .into_iter()
        .enumerate()
    {
        for x in 0..8 {
            let color = if bits & (0x80 >> x) != 0 {
                [255, 255, 255, 255]
            } else {
                [170, 85, 0, 255]
            };
            assert_eq!(pixel(&pixels, x, y), color);
        }
    }
    assert_eq!(pixel(&pixels, 8, 0), [0, 0, 0, 255]);
    assert_eq!(pixel(&pixels, 639, 199), [85, 85, 255, 255]);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn cp437_controls_and_box_drawing_are_glyphs() {
    let mut vga = text_device();
    for (cell, character) in [1, 0xc4, 0xb3].into_iter().enumerate() {
        set_cell(&mut vga, cell, character, 7);
    }
    let pixels = render(&vga.snapshot());
    assert_eq!(pixel(&pixels, 1, 0), [170, 170, 170, 255]);
    for x in 8..16 {
        assert_eq!(pixel(&pixels, x, 4), [170, 170, 170, 255]);
    }
    for y in 0..8 {
        assert_eq!(pixel(&pixels, 19, y), [170, 170, 170, 255]);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn blinking_uses_the_captured_frame_phase() {
    let mut vga = text_device();
    set_cell(&mut vga, 0, 0xdb, 0x94);
    set_cell(&mut vga, 1, 0xdb, 0x14);
    let mut snapshot = vga.snapshot();
    for (blink_on, blinking_color) in [(true, [170, 0, 0, 255]), (false, [0, 0, 170, 255])] {
        snapshot.blink_on = blink_on;
        let pixels = render(&snapshot);
        assert_eq!(pixel(&pixels, 0, 0), blinking_color);
        assert_eq!(pixel(&pixels, 8, 0), [170, 0, 0, 255]);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn cursor_shape_visibility_and_blink() {
    let mut snapshot = text_device().snapshot();
    snapshot.registers.crtc.values[14] = 0;
    snapshot.registers.crtc.values[15] = 82;
    snapshot.registers.crtc.values[10] = 6;
    snapshot.registers.crtc.values[11] = 7;
    let pixels = render(&snapshot);
    assert_eq!(pixel(&pixels, 16, 13), [0, 0, 0, 255]);
    assert_eq!(pixel(&pixels, 16, 14), [170, 170, 170, 255]);
    assert_eq!(pixel(&pixels, 23, 15), [170, 170, 170, 255]);
    snapshot.blink_on = false;
    assert_eq!(pixel(&render(&snapshot), 16, 14), [0, 0, 0, 255]);
    snapshot.blink_on = true;
    for (address, start, end) in [(82u16, 0x26, 7), (82, 7, 6), (82, 8, 31), (2002, 6, 7)] {
        let [high, low] = address.to_be_bytes();
        snapshot.registers.crtc.values[14] = high;
        snapshot.registers.crtc.values[15] = low;
        snapshot.registers.crtc.values[10] = start;
        snapshot.registers.crtc.values[11] = end;
        assert!(render(&snapshot)
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| *p == [0, 0, 0, 255]));
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn cursor_follows_display_start_address_wrap_and_skew() {
    let mut snapshot = text_device().snapshot();
    snapshot.registers.crtc.values[12] = 255;
    snapshot.registers.crtc.values[13] = 255;
    snapshot.registers.crtc.values[10] = 6;
    snapshot.registers.crtc.values[11] = 7;
    let pixels = render(&snapshot);
    assert_eq!(pixel(&pixels, 0, 6), [0, 0, 0, 255]);
    assert_eq!(pixel(&pixels, 8, 6), [170, 170, 170, 255]);
    snapshot.registers.crtc.values[11] = 0x27;
    let pixels = render(&snapshot);
    assert_eq!(pixel(&pixels, 8, 6), [0, 0, 0, 255]);
    assert_eq!(pixel(&pixels, 16, 6), [170, 170, 170, 255]);
    snapshot.registers.crtc.values[12] = 0;
    snapshot.registers.crtc.values[13] = 0;
    for address in [79u16, 0xffff] {
        let [high, low] = address.to_be_bytes();
        snapshot.registers.crtc.values[14] = high;
        snapshot.registers.crtc.values[15] = low;
        assert!(render(&snapshot)
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| *p == [0, 0, 0, 255]));
    }
}

#[path = "programmed_text.rs"]
mod programmed;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn programmable_display_matches_literal_pixels() {
    for case in programmed::cases() {
        assert_eq!(case.display.geometry().size, case.size, "{}", case.name);
        let pixels = render(&case.display);
        for (x, y, expected) in case.pixels {
            let offset = ((y * case.size[0] + x) * 4) as usize;
            assert_eq!(
                pixels[offset..offset + 4],
                expected,
                "{} ({x},{y})",
                case.name
            );
        }
    }
}
