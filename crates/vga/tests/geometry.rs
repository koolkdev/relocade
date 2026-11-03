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
    assert_eq!(preset.geometry().size, [656, 414]);
    assert_eq!(preset.geometry().raster_origin, [792, 442]);
    assert_eq!(preset.geometry().active_size, [640, 400]);
    assert_eq!(preset.geometry().pixel_aspect_ratio(), (5, 6));
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
        assert_eq!(vga.snapshot().geometry().active_size, size);
    }
    assert_eq!(preset.geometry().size, [656, 414]);
}

// Remove borders so these fixtures isolate repeated-row and split alignment.
fn active_raster(vga: &mut Vga<ManualClock>, lines: u8) {
    for (index, value) in [
        (0, 95),
        (2, 79),
        (3, 0x83),
        (5, 0x80),
        (6, lines),
        (0x15, lines - 1),
        (0x16, lines + 1),
    ] {
        crtc(vga, index, value);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn doubled_rows_share_storage_unless_a_split_separates_them() {
    let mut vga = Vga::with_clock(Rc::new(ManualClock::default()));
    for (index, value) in [(0x11, 0), (7, 0), (9, 0x81), (0x12, 3)] {
        crtc(&mut vga, index, value);
    }
    active_raster(&mut vga, 4);
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
    for shift in [0, 0x20, 0x40] {
        let mut vga = Vga::with_clock(Rc::new(ManualClock::default()));
        if shift == 0x40 {
            common::support::indexed256::program(&mut vga);
        } else {
            common::support::planar16::program(&mut vga);
            common::support::registers::indexed(&mut vga, 0x3ce, 5, shift);
        }
        let width = if shift == 0x40 { 320 } else { 640 };
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
            active_raster(&mut vga, last + 1);
            let geometry = vga.snapshot().geometry();
            assert_eq!(geometry.size, [width, expected_height]);
            assert_eq!(geometry.scanline_step, expected_step);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn planar_geometry_uses_character_clocks_and_double_scanning() {
    let mut vga = Vga::with_clock(Rc::new(ManualClock::default()));
    common::support::planar16::program(&mut vga);
    assert_eq!(vga.snapshot().geometry().size, [656, 496]);
    assert_eq!(vga.snapshot().geometry().raster_origin, [792, 517]);
    assert_eq!(vga.snapshot().geometry().pixel_aspect_ratio(), (1, 1));
    for (index, value) in [
        (0, 0x2d),
        (1, 39),
        (2, 40),
        (3, 0x90),
        (5, 0x80),
        (6, 0xbf),
        (7, 0x1f),
        (9, 0xc0),
        (0x12, 0x8f),
        (0x15, 0x96),
        (0x16, 0xb9),
    ] {
        crtc(&mut vga, index, value);
    }
    // Divide Dot Clock changes the oscillator rate, not the pixel count.
    for clocking in [1, 9] {
        common::support::registers::indexed(&mut vga, 0x3c4, 1, clocking);
        let geometry = vga.snapshot().geometry();
        assert_eq!(geometry.size, [336, 414]);
        assert_eq!(geometry.active_size, [320, 400]);
        assert_eq!(geometry.scanline_step, 1);
        assert_eq!(geometry.pixel_aspect_ratio(), (5, 3));
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn nonblank_window_follows_counter_wrap_and_full_blanking() {
    let mut vga = Vga::with_clock(Rc::new(ManualClock::default()));
    for (index, value) in [
        (0x11, 0),
        (0, 5),
        (1, 1),
        (2, 3),
        (3, 0x87),
        (5, 0),
        (6, 8),
        (7, 0),
        (9, 0),
        (0x12, 3),
        (0x15, 5),
        (0x16, 7),
    ] {
        crtc(&mut vga, index, value);
    }
    let geometry = vga.snapshot().geometry();
    assert_eq!(geometry.size, [48, 8]);
    assert_eq!(geometry.raster_origin, [64, 8]);
    assert_eq!(geometry.active_size, [16, 4]);
    assert_eq!(geometry.pixel_aspect_ratio(), (1, 3));

    // Blanking across the counter reset leaves one contiguous interior span.
    crtc(&mut vga, 2, 7);
    crtc(&mut vga, 3, 0x81);
    assert_eq!(vga.snapshot().geometry().size, [48, 8]);
    assert_eq!(vga.snapshot().geometry().raster_origin, [16, 8]);

    // An unreachable start exposes the whole raster, not just display enable.
    crtc(&mut vga, 2, 255);
    crtc(&mut vga, 0x15, 255);
    assert_eq!(vga.snapshot().geometry().size, [80, 10]);
    assert_eq!(vga.snapshot().geometry().raster_origin, [0, 0]);

    // An unreachable end holds blanking across the whole raster.
    crtc(&mut vga, 2, 3);
    crtc(&mut vga, 3, 0x83);
    assert_eq!(vga.snapshot().geometry().size, [1, 1]);
    assert_eq!(vga.snapshot().geometry().raster_origin, [0, 0]);
}
