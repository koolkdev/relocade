use super::reference::render;
const WIDTH: u32 = 640;
use relocade_vga::{TextCell, TextCursor, TextScreen};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

fn blank_screen() -> TextScreen {
    TextScreen {
        cells: [TextCell {
            character: b' ',
            attribute: 7,
        }; TextScreen::COLUMNS * TextScreen::ROWS],
        cursor: TextCursor {
            address: 0,
            start_scan_line: 0x20,
            end_scan_line: 7,
        },
        start_address: 0,
    }
}

fn pixel(pixels: &[u8], x: usize, y: usize) -> &[u8] {
    let start = (y * WIDTH as usize + x) * 4;
    &pixels[start..start + 4]
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn glyph_orientation_colors_and_last_cell() {
    let mut screen = blank_screen();
    screen.cells[0] = TextCell {
        character: b'F',
        attribute: 0x6f, // White on brown.
    };
    screen.cells[1999] = TextCell {
        character: 0xdb, // CP437 full block.
        attribute: 0x09,
    };
    let pixels = render(&screen, true, false);
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
    let mut screen = blank_screen();
    for (cell, character) in screen.cells.iter_mut().zip([1, 0xc4, 0xb3]) {
        cell.character = character;
    }
    let pixels = render(&screen, true, false);
    assert_eq!(pixel(&pixels, 1, 0), [170, 170, 170, 255]); // Smiley, not a control.
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
    let mut screen = blank_screen();
    screen.cells[0] = TextCell {
        character: 0xdb,
        attribute: 0x94, // Blinking red on blue, not bright blue background.
    };
    screen.cells[1] = TextCell {
        character: 0xdb,
        attribute: 0x14,
    };
    for (blink_on, blinking_color) in [(true, [170, 0, 0, 255]), (false, [0, 0, 170, 255])] {
        let pixels = render(&screen, blink_on, false);
        assert_eq!(pixel(&pixels, 0, 0), blinking_color);
        assert_eq!(pixel(&pixels, 8, 0), [170, 0, 0, 255]);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn cursor_shape_visibility_and_blink() {
    let mut screen = blank_screen();
    screen.cursor = TextCursor {
        address: 82,
        start_scan_line: 6,
        end_scan_line: 7,
    };
    let pixels = render(&screen, true, false);
    assert_eq!(pixel(&pixels, 16, 13), [0, 0, 0, 255]);
    assert_eq!(pixel(&pixels, 16, 14), [170, 170, 170, 255]);
    assert_eq!(pixel(&pixels, 23, 15), [170, 170, 170, 255]);
    assert_eq!(
        pixel(&render(&screen, false, false), 16, 14),
        [0, 0, 0, 255]
    );
    for (address, start, end) in [
        (82, 0x26, 7), // Disabled.
        (82, 7, 6),    // Reversed.
        (82, 8, 31),   // Below the character.
        (2002, 6, 7),  // Below the screen.
    ] {
        screen.cursor = TextCursor {
            address,
            start_scan_line: start,
            end_scan_line: end,
        };
        let pixels = render(&screen, true, false);
        assert!(pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| *p == [0, 0, 0, 255]));
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn cursor_follows_display_start_address_wrap_and_skew() {
    let mut screen = blank_screen();
    screen.start_address = 0xffff;
    screen.cursor = TextCursor {
        address: 0,
        start_scan_line: 6,
        end_scan_line: 7,
    };
    let pixels = render(&screen, true, false);
    assert_eq!(pixel(&pixels, 0, 6), [0, 0, 0, 255]);
    assert_eq!(pixel(&pixels, 8, 6), [170, 170, 170, 255]);
    screen.cursor.end_scan_line = 0x27; // Delay the cursor by one cell.
    let pixels = render(&screen, true, false);
    assert_eq!(pixel(&pixels, 8, 6), [0, 0, 0, 255]);
    assert_eq!(pixel(&pixels, 16, 6), [170, 170, 170, 255]);
    // A skew into horizontal blanking does not reappear on the next row.
    screen.start_address = 0;
    for address in [79, 0xffff] {
        screen.cursor.address = address;
        let pixels = render(&screen, true, false);
        assert!(pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| *p == [0, 0, 0, 255]));
    }
}
