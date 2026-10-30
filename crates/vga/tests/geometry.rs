mod common;

use common::{crtc, ManualClock};
use relocade_machine::Device;
use relocade_vga::Vga;
use std::rc::Rc;

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn dimensions_follow_captured_crtc_and_character_clock() {
    let mut vga = Vga::with_clock(Rc::new(ManualClock::default()));
    let preset = vga.snapshot();
    assert_eq!(preset.geometry().size, [640, 200]);
    assert_eq!(preset.geometry().pixel_aspect_ratio(), (5, 12));
    crtc(&mut vga, 0x11, 0); // Unlock geometry.
    crtc(&mut vga, 9, 0);
    for (columns, clocking, overflow, last_line, size) in [
        (39, 0u8, 0, 7, [360, 8]),
        (255, 0, 0x42, 255, [2304, 1024]),
        (0, 1, 0, 0, [8, 1]),
    ] {
        crtc(&mut vga, 1, columns);
        crtc(&mut vga, 7, overflow);
        crtc(&mut vga, 0x12, last_line);
        vga.write_port(0x3c4, 2, u32::from(clocking) << 8 | 1);
        assert_eq!(vga.snapshot().geometry().size, size);
    }
    assert_eq!(preset.geometry().size, [640, 200]);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn doubled_rows_share_storage_unless_a_split_separates_them() {
    let mut vga = Vga::with_clock(Rc::new(ManualClock::default()));
    for (index, value) in [(0x11, 0), (7, 0), (9, 0x81), (0x12, 3)] {
        crtc(&mut vga, index, value);
    }
    for (split, height, step) in [(0, 4, 1), (1, 2, 2), (2, 4, 1), (3, 2, 2)] {
        crtc(&mut vga, 0x18, split);
        let geometry = vga.snapshot().geometry();
        assert_eq!(geometry.size, [640, height]);
        assert_eq!(geometry.scanline_step, step);
    }
    crtc(&mut vga, 0x18, 0);
    crtc(&mut vga, 7, 0x10); // The split is now below the visible image.
    assert_eq!(vga.snapshot().geometry().size, [640, 2]);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn graphics_geometry_preserves_partial_repeats_and_row_address_changes() {
    let mut vga = Vga::with_clock(Rc::new(ManualClock::default()));
    common::support::indexed256::program(&mut vga);
    assert_eq!(vga.snapshot().geometry().size, [320, 200]);
    assert_eq!(vga.snapshot().geometry().pixel_aspect_ratio(), (5, 6));
    // Four row scans per memory row, eight visible scanlines.
    crtc(&mut vga, 7, 0x10);
    crtc(&mut vga, 9, 0x43);
    crtc(&mut vga, 0x12, 7);
    for (preset, split, mode, last, expected_height, expected_step) in [
        (0, 0xff, 0xa3, 7, 2, 4),
        (1, 0xff, 0xa3, 7, 8, 1), // Preset starts partway through a repeated row.
        (0, 2, 0xa3, 7, 8, 1),    // Split cuts a group of four.
        (0, 3, 0xa3, 7, 2, 4),    // Split lies exactly between groups.
        (0, 0xff, 0xa1, 7, 8, 1), // Row scan substitutes address bit 14.
        (0, 0xff, 0xa3, 5, 6, 1), // A partial final group has a different height.
    ] {
        crtc(&mut vga, 7, 0); // Keep the line compare in the low eight bits.
        crtc(&mut vga, 9, 3);
        crtc(&mut vga, 8, preset);
        crtc(&mut vga, 0x18, split);
        crtc(&mut vga, 0x17, mode);
        crtc(&mut vga, 0x12, last);
        let geometry = vga.snapshot().geometry();
        assert_eq!(geometry.size, [320, expected_height]);
        assert_eq!(geometry.scanline_step, expected_step);
    }
}
