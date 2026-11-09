use relocade_clock_test_support::{origin as clock_origin, ManualClock};
mod common;

use common::boundary;
use relocade_machine::Device;
use relocade_vga::Vga;

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

fn indexed(vga: &mut impl Device, port: u16, index: u8, value: u8) {
    vga.write_port(port, 2, u32::from(u16::from_le_bytes([index, value])));
}

fn font_access(vga: &mut impl Device) {
    indexed(vga, 0x3c4, 4, 6);
    indexed(vga, 0x3c4, 2, 4);
    indexed(vga, 0x3ce, 5, 0);
    indexed(vga, 0x3ce, 6, 5);
    indexed(vga, 0x3ce, 4, 2);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn snapshots_own_fonts_palette_and_programmed_geometry_after_device_destruction() {
    let mut vga = Vga::with_clock(ManualClock::new().clock());
    font_access(&mut vga);
    vga.write_mmio(0xae821, 1, 0xa5); // A glyph row in the last font bank.
    indexed(&mut vga, 0x3c4, 3, 0x35);
    indexed(&mut vga, 0x3d4, 0x11, 0); // Unlock geometry registers.
    indexed(&mut vga, 0x3d4, 1, 39);
    indexed(&mut vga, 0x3d4, 9, 15);
    indexed(&mut vga, 0x3d4, 0x13, 20);
    vga.write_port(0x3c0, 1, 0x12);
    vga.write_port(0x3c0, 1, 5);
    vga.write_port(0x3c0, 1, 0x34); // Enable display independently of the CPU index.
    vga.write_port(0x3da, 1, 0x5a);
    vga.write_port(0x3c6, 1, 0x7f);
    vga.write_port(0x3c8, 1, 123);
    for component in [63, 21, 7] {
        vga.write_port(0x3c9, 1, component);
    }
    let snapshot = vga.snapshot();
    vga.write_mmio(0xae821, 1, 0);
    indexed(&mut vga, 0x3d4, 1, 79);
    vga.write_port(0x3da, 1, 0xa5);
    vga.write_port(0x3c8, 1, 123);
    vga.write_port(0x3c9, 1, 0);
    drop(vga);

    assert_eq!(snapshot.plane(2).len(), 65536);
    assert_eq!(snapshot.plane(2)[0xe821], 0xa5);
    assert_eq!(snapshot.geometry().size, [320, 400]);
    let state = snapshot.vga().unwrap();
    assert_eq!(state.sequencer[3], 0x35);
    assert_eq!(state.crtc.values[1], 39);
    assert_eq!(state.crtc.values[9], 15);
    assert_eq!(state.crtc.values[0x13], 20);
    assert_eq!(state.attributes[0x12], 5);
    assert!(state.display_enabled);
    assert_eq!(snapshot.dac.mask, 0x7f);
    assert_eq!(snapshot.dac.colors[123], [63, 21, 7]);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn immediate_snapshots_preserve_cpu_latches_and_pending_port_transfers() {
    let mut vga = Vga::with_clock(ManualClock::new().clock());
    let source = vga.frame_source();
    font_access(&mut vga);
    vga.write_mmio(0xa0010, 1, 0x5a);
    vga.write_mmio(0xa0020, 1, 0xa5);
    assert_eq!(vga.read_mmio(0xa0010, 1), 0x5a);
    vga.write_port(0x3c0, 1, 4); // Pending Attribute data write.
    vga.write_port(0x3c8, 1, 42);
    vga.write_port(0x3c9, 1, 11); // Pending green and blue components.
    let registers = vga.registers().clone();
    let snapshot = vga.snapshot();
    assert_eq!(vga.registers(), &registers);
    assert!(source.current_capture().is_none());
    assert_eq!(snapshot.plane(2)[0x20], 0xa5);
    vga.write_port(0x3c0, 1, 0x31);
    assert_eq!(vga.read_port(0x3c1, 1), 0x31);
    vga.write_port(0x3c9, 1, 22);
    vga.write_port(0x3c9, 1, 33);
    assert_eq!(vga.snapshot().dac.colors[42], [11, 22, 33]);
    assert!(!snapshot.vga().unwrap().display_enabled);
    assert_eq!(snapshot.dac.colors[42][0], 11);
    indexed(&mut vga, 0x3ce, 5, 1); // Copy the original read latch.
    vga.write_mmio(0xa0020, 1, 0);
    assert_eq!(vga.read_mmio(0xa0020, 1), 0x5a);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn newest_capture_contains_changes_from_skipped_frames() {
    let clock = ManualClock::default();
    let (vga, mut video) = common::start_video(Vga::with_clock(clock.clock()));
    let source = vga.borrow().frame_source();
    font_access(&mut *vga.borrow_mut());
    vga.borrow_mut().write_mmio(0xa0100, 1, 0x12);
    clock.advance_to(clock_origin() + boundary(1));
    vga.borrow_mut().read_port(0x3da, 1);
    common::service_video(video.as_mut());
    let first = source.current_capture().unwrap();
    vga.borrow_mut().write_mmio(0xa0200, 1, 0x34);
    clock.advance_to(clock_origin() + boundary(2));
    vga.borrow_mut().read_port(0x3da, 1);
    common::service_video(video.as_mut());
    let second = source.clone().current_capture().unwrap(); // An independent reader.
    vga.borrow_mut().write_mmio(0xa0300, 1, 0x56);
    clock.advance_to(clock_origin() + boundary(3));
    vga.borrow_mut().read_port(0x3da, 1);
    common::service_video(video.as_mut());
    let newest = source.current_capture().unwrap();
    drop(video);
    drop(vga);
    assert_eq!(newest.sequence, 3);
    assert_eq!(first.display.plane(2)[0x200], 0);
    assert_eq!(second.display.plane(2)[0x200], 0x34);
    assert_eq!(second.display.plane(2)[0x300], 0);
    assert_eq!(newest.display.plane(2)[0x100], 0x12);
    assert_eq!(newest.display.plane(2)[0x200], 0x34);
    assert_eq!(newest.display.plane(2)[0x300], 0x56);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn snapshots_share_planes_until_a_final_physical_byte_changes() {
    let mut vga = Vga::with_clock(ManualClock::new().clock());
    font_access(&mut vga);
    indexed(&mut vga, 0x3c4, 2, 0x0f);
    indexed(&mut vga, 0x3c4, 4, 0x0e); // Chain four selects plane one below.
    let before = vga.snapshot();
    let cloned = before.clone();
    for plane in 0..4 {
        assert!(std::ptr::eq(before.plane(plane), cloned.plane(plane)));
    }

    vga.write_mmio(0xa0125, 1, 0x5a);
    let changed = vga.snapshot();
    assert_eq!(before.plane(1)[0x124], 0);
    assert_eq!(changed.plane(1)[0x124], 0x5a);
    assert!(!std::ptr::eq(before.plane(1), changed.plane(1)));
    for plane in [0, 2, 3] {
        assert!(std::ptr::eq(before.plane(plane), changed.plane(plane)));
    }

    vga.write_mmio(0xa0125, 1, 0x5a); // Same byte.
    vga.read_mmio(0xa0125, 1); // Load the existing byte into the latch.
    indexed(&mut vga, 0x3ce, 8, 0); // Preserve every latched bit.
    vga.write_mmio(0xa0125, 1, 0xff);
    indexed(&mut vga, 0x3c4, 2, 0); // Reject the write entirely.
    vga.write_mmio(0xa0125, 1, 0);
    let unchanged = vga.snapshot();
    for plane in 0..4 {
        assert!(std::ptr::eq(changed.plane(plane), unchanged.plane(plane)));
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn packed_snapshots_retain_pixels_palette_and_layout_across_mode_changes() {
    use common::support::{registers::palette, svga};
    use relocade_vga::Scanout;

    let mut vga = Vga::with_clock(ManualClock::new().clock());
    svga::enable(&mut vga);
    for (address, value) in [
        (0, 1),
        (0xffff, 2),
        (0x10000, 3),
        (0x3ffff, 4),
        (0x40000, 5),
        (307199, 6),
    ] {
        svga::write_byte(&mut vga, address, value);
    }
    palette(&mut vga, 5, [21, 42, 63]);
    vga.write_port(0x3c6, 1, 0x7f);
    let before = vga.snapshot();
    let Scanout::Svga(layout) = *before.scanout() else {
        panic!("expected packed scanout")
    };
    assert_eq!(layout.size(), [640, 480]);
    assert_eq!(layout.pitch(), 640);
    assert_eq!(layout.display_start(), 0);
    assert_eq!(before.geometry().size, [640, 480]);
    assert_eq!(before.geometry().pixel_aspect_ratio(), (1, 1));
    assert_eq!(before.vram_byte(0x80000), None);
    assert_eq!(before.vram_byte(u32::MAX), None);

    svga::write_byte(&mut vga, 0x40000, 7);
    palette(&mut vga, 5, [0, 0, 0]);
    svga::set(&mut vga, 4, 0);
    assert!(matches!(vga.snapshot().scanout(), Scanout::Vga(_)));
    svga::enable(&mut vga); // Mode entry clears current VRAM, including extended blocks.
    assert_eq!(vga.snapshot().vram_byte(0x40000), Some(0));
    drop(vga);

    for (address, value) in [
        (0, 1),
        (0xffff, 2),
        (0x10000, 3),
        (0x3ffff, 4),
        (0x40000, 5),
        (307199, 6),
    ] {
        assert_eq!(before.vram_byte(address), Some(value));
    }
    assert_eq!(before.dac.colors[5], [21, 42, 63]);
    assert_eq!(before.dac.mask, 0x7f);
    assert_eq!(before.scanout(), &Scanout::Svga(layout));
}
