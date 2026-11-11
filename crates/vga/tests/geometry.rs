use crate::common::fixture::ConnectedVga;
use relocade_clock_test_support::ManualClock;
mod common;

use common::crtc;

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn dimensions_follow_captured_crtc_and_character_clock() {
    let vga = ConnectedVga::new(ManualClock::new().clock());
    let preset = vga.snapshot();
    assert_eq!(preset.geometry().size, [640, 200]);
    assert_eq!(preset.geometry().pixel_aspect_ratio(), (5, 12));
    crtc(&vga.io, 0x11, 0); // Unlock geometry.
    crtc(&vga.io, 9, 0);
    for (columns, clocking, overflow, last_line, size) in [
        (39, 0u8, 0, 7, [360, 8]),
        (255, 0, 0x42, 255, [2304, 1024]),
        (0, 1, 0, 0, [8, 1]),
    ] {
        crtc(&vga.io, 1, columns);
        crtc(&vga.io, 7, overflow);
        crtc(&vga.io, 0x12, last_line);
        vga.io.write_port(0x3c4, 2, u32::from(clocking) << 8 | 1);
        assert_eq!(vga.snapshot().geometry().size, size);
    }
    assert_eq!(preset.geometry().size, [640, 200]);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn doubled_rows_share_storage_unless_a_split_separates_them() {
    let vga = ConnectedVga::new(ManualClock::new().clock());
    for (index, value) in [(0x11, 0), (7, 0), (9, 0x81), (0x12, 3)] {
        crtc(&vga.io, index, value);
    }
    for (split, height, step) in [(0, 4, 1), (1, 2, 2), (2, 4, 1), (3, 2, 2)] {
        crtc(&vga.io, 0x18, split);
        let geometry = vga.snapshot().vga().unwrap().geometry();
        assert_eq!(geometry.output.size, [640, height]);
        assert_eq!(geometry.scanline_step, step);
    }
    crtc(&vga.io, 0x18, 0);
    crtc(&vga.io, 7, 0x10); // The split is now below the visible image.
    assert_eq!(vga.snapshot().geometry().size, [640, 2]);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn graphics_geometry_preserves_partial_repeats_and_row_address_changes() {
    for shift in [0, 0x20, 0x40] {
        let vga = ConnectedVga::new(ManualClock::new().clock());
        if shift == 0x40 {
            common::support::indexed256::program(&vga.io);
        } else {
            common::support::planar16::program(&vga.io);
            common::support::registers::indexed(&vga.io, 0x3ce, 5, shift);
        }
        let width = if shift == 0x40 { 320 } else { 640 };
        // Four row scans per memory row, eight visible scanlines.
        crtc(&vga.io, 7, 0x10);
        crtc(&vga.io, 9, 0x43);
        crtc(&vga.io, 0x12, 7);
        for (preset, split, mode, last, expected_height, expected_step) in [
            (0, 0xff, 0xa3, 7, 2, 4),
            (1, 0xff, 0xa3, 7, 8, 1), // Preset starts partway through a repeated row.
            (0, 2, 0xa3, 7, 8, 1),    // Split cuts a group of four.
            (0, 3, 0xa3, 7, 2, 4),    // Split lies exactly between groups.
            (0, 0xff, 0xa1, 7, 8, 1), // Row scan substitutes address bit 14.
            (0, 0xff, 0xa3, 5, 6, 1), // A partial final group has a different height.
        ] {
            crtc(&vga.io, 7, 0); // Keep the line compare in the low eight bits.
            crtc(&vga.io, 9, 3);
            crtc(&vga.io, 8, preset);
            crtc(&vga.io, 0x18, split);
            crtc(&vga.io, 0x17, mode);
            crtc(&vga.io, 0x12, last);
            let geometry = vga.snapshot().vga().unwrap().geometry();
            assert_eq!(geometry.output.size, [width, expected_height]);
            assert_eq!(geometry.scanline_step, expected_step);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn planar_geometry_uses_character_clocks_and_double_scanning() {
    let vga = ConnectedVga::new(ManualClock::new().clock());
    common::support::planar16::program(&vga.io);
    assert_eq!(vga.snapshot().geometry().size, [640, 480]);
    assert_eq!(
        vga.device
            .borrow()
            .snapshot()
            .geometry()
            .pixel_aspect_ratio(),
        (1, 1)
    );
    for (index, value) in [(1, 39), (7, 0x1f), (9, 0xc0), (0x12, 0x8f)] {
        crtc(&vga.io, index, value);
    }
    // Divide Dot Clock changes the oscillator rate, not the pixel count.
    for clocking in [1, 9] {
        common::support::registers::indexed(&vga.io, 0x3c4, 1, clocking);
        let geometry = vga.snapshot().vga().unwrap().geometry();
        assert_eq!(geometry.output.size, [320, 200]);
        assert_eq!(geometry.scanline_step, 2);
        assert_eq!(geometry.output.pixel_aspect_ratio(), (5, 6));
    }
}
