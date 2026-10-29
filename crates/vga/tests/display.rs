mod common;

use common::{crtc, ManualClock};
use relocade_machine::Device;
use relocade_vga::Vga;
use std::rc::Rc;

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

fn attribute(vga: &mut impl Device, index: u8, value: u8) {
    vga.read_port(0x3da, 1);
    vga.write_port(0x3c0, 1, u32::from(index));
    vga.write_port(0x3c0, 1, u32::from(value));
    vga.write_port(0x3c0, 1, 0x20);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn font_and_color_selection_follow_captured_registers() {
    let mut vga = Vga::with_clock(Rc::new(ManualClock::default()));
    vga.write_port(0x3c4, 2, 0x1303); // Map A at zero, map B at E000.
    attribute(&mut vga, 0x10, 0x84);
    attribute(&mut vga, 0x14, 0x0a);
    vga.write_port(0x3c6, 1, 0x7f);
    vga.write_port(0x3c8, 1, 0x21);
    for component in [1, 17, 63] {
        vga.write_port(0x3c9, 1, component);
    }
    let snapshot = vga.snapshot();
    let text = snapshot.text_display();
    assert!(text.dual_font);
    assert_eq!((text.font_a, text.font_b), (0, 0xe000));
    assert_eq!(text.font_address(b'A', 7, 15), 0xe82f);
    assert_eq!(text.font_address(b'A', 15, 15), 0x082f);
    assert_eq!(text.colors[1].to_le_bytes(), [4, 69, 255, 255]);

    // Disabling extended memory aliases the bank and disables dual-font selection.
    vga.write_port(0x3c4, 2, 0x0004);
    let text = vga.snapshot().text_display();
    assert!(!text.dual_font);
    assert_eq!(text.font_address(b'A', 15, 15), 0x282f);
    assert_eq!(snapshot.text_display().font_b, 0xe000);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn scanlines_apply_stride_preset_panning_and_split_address_reset() {
    let mut vga = Vga::with_clock(Rc::new(ManualClock::default()));
    for (register, value) in [
        (0x11, 0), // Unlock geometry.
        (1, 39),
        (7, 0),
        (9, 3),
        (0x12, 7),
        (0x13, 21),
        (0x0c, 0x12),
        (0x0d, 0x34),
        (8, 0x21), // One preset row and one byte of panning.
        (0x18, 3),
    ] {
        crtc(&mut vga, register, value);
    }
    vga.write_port(0x3c4, 2, 0x0001); // Nine-dot characters.
    attribute(&mut vga, 0x13, 2);
    attribute(&mut vga, 0x10, 0x2c); // Reset pel panning below the split.
    let layout = vga.snapshot().text_display().layout;
    assert_eq!(layout.size, [360, 8]);
    for (y, counter, glyph_row, pan, address) in [
        (0, 0x1235, 1, 3, 0x246a),
        (3, 0x125f, 0, 3, 0x24be),
        (4, 1, 0, 0, 2),
    ] {
        let line = layout.scanline(y);
        assert_eq!(
            (line.counter, line.glyph_row, line.pan),
            (counter, glyph_row, pan)
        );
        assert_eq!(layout.address(line.counter, line.glyph_row), address);
    }
}
