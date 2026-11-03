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

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn image_rows_preserve_physical_scanlines_in_text_and_graphics() {
    for layout in 0..3 {
        let mut vga = Vga::with_clock(Rc::new(ManualClock::default()));
        match layout {
            0 => {} // Device text preset.
            1 => common::support::planar16::program(&mut vga),
            2 => common::support::indexed256::program(&mut vga),
            _ => unreachable!(),
        }
        // Eight physical scanlines, no borders. Row height and double-scanning
        // affect pixel contents, not the image's physical dimensions.
        for (index, value) in [
            (0x11, 0),
            (0, 95),
            (2, 79),
            (3, 0x83),
            (5, 0x80),
            (6, 8),
            (7, 0),
            (8, 0),
            (0x12, 7),
            (0x15, 7),
            (0x16, 9),
            (0x18, 255),
        ] {
            crtc(&mut vga, index, value);
        }
        let width = if layout == 2 { 320 } else { 640 };
        for maximum_scan_line in [0, 3, 0x81] {
            crtc(&mut vga, 9, maximum_scan_line);
            let geometry = vga.snapshot().geometry();
            assert_eq!(geometry.size, [width, 8]);
            assert_eq!(geometry.active_size, [width, 8]);
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
