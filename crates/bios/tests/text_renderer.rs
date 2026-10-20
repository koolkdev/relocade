use relocade_bios::{TextCell, TextCursor, TextRenderer, TextScreen};
use std::time::Duration;

fn blank_screen() -> TextScreen {
    TextScreen {
        cells: [TextCell {
            character: b' ',
            attribute: 7,
        }; TextScreen::COLUMNS * TextScreen::ROWS],
        cursor: TextCursor {
            row: 0,
            column: 0,
            start_scan_line: 0x20,
            end_scan_line: 7,
        },
        page: 0,
    }
}

fn pixel(pixels: &[u8], x: usize, y: usize) -> &[u8] {
    let start = (y * TextRenderer::WIDTH as usize + x) * 4;
    &pixels[start..start + 4]
}

#[test]
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
    let mut renderer = TextRenderer::new();
    let pixels = renderer.render(&screen, Duration::ZERO);
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
            assert_eq!(pixel(pixels, x, y), color);
        }
    }
    assert_eq!(pixel(pixels, 8, 0), [0, 0, 0, 255]);
    assert_eq!(pixel(pixels, 639, 199), [85, 85, 255, 255]);
}

#[test]
fn cp437_controls_and_box_drawing_are_glyphs() {
    let mut screen = blank_screen();
    for (cell, character) in screen.cells.iter_mut().zip([1, 0xc4, 0xb3]) {
        cell.character = character;
    }
    let mut renderer = TextRenderer::new();
    let pixels = renderer.render(&screen, Duration::ZERO);
    assert_eq!(pixel(pixels, 1, 0), [170, 170, 170, 255]); // Smiley, not a control.
    for x in 8..16 {
        assert_eq!(pixel(pixels, x, 4), [170, 170, 170, 255]);
    }
    for y in 0..8 {
        assert_eq!(pixel(pixels, 19, y), [170, 170, 170, 255]);
    }
}

#[test]
fn blinking_uses_presentation_time_and_reuses_pixels() {
    let mut screen = blank_screen();
    screen.cells[0] = TextCell {
        character: 0xdb,
        attribute: 0x94, // Blinking red on blue, not bright blue background.
    };
    screen.cells[1] = TextCell {
        character: 0xdb,
        attribute: 0x14,
    };
    let mut renderer = TextRenderer::new();
    let allocation = renderer.render(&screen, Duration::ZERO).as_ptr();
    for (millis, blinking_color) in [
        (499, [170, 0, 0, 255]),
        (500, [0, 0, 170, 255]),
        (999, [0, 0, 170, 255]),
        (1000, [170, 0, 0, 255]),
    ] {
        let pixels = renderer.render(&screen, Duration::from_millis(millis));
        assert_eq!(pixels.as_ptr(), allocation);
        assert_eq!(pixel(pixels, 0, 0), blinking_color);
        assert_eq!(pixel(pixels, 8, 0), [170, 0, 0, 255]);
    }
}

#[test]
fn cursor_shape_visibility_and_blink() {
    let mut screen = blank_screen();
    screen.cursor = TextCursor {
        row: 1,
        column: 2,
        start_scan_line: 6,
        end_scan_line: 7,
    };
    let mut renderer = TextRenderer::new();
    let pixels = renderer.render(&screen, Duration::ZERO);
    assert_eq!(pixel(pixels, 16, 13), [0, 0, 0, 255]);
    assert_eq!(pixel(pixels, 16, 14), [170, 170, 170, 255]);
    assert_eq!(pixel(pixels, 23, 15), [170, 170, 170, 255]);
    assert_eq!(
        pixel(renderer.render(&screen, Duration::from_millis(500)), 16, 14),
        [0, 0, 0, 255]
    );
    for (row, column, start, end) in [
        (1, 2, 0x26, 7), // Disabled.
        (1, 2, 7, 6),    // Reversed.
        (1, 2, 8, 31),   // Below the character.
        (25, 2, 6, 7),   // Below the screen.
        (1, 80, 6, 7),   // Outside the row; must not wrap.
    ] {
        screen.cursor = TextCursor {
            row,
            column,
            start_scan_line: start,
            end_scan_line: end,
        };
        let pixels = renderer.render(&screen, Duration::ZERO);
        assert!(pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| *p == [0, 0, 0, 255]));
    }
}
