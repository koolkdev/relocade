mod common;

use common::{boundary, ManualClock};
use relocade_machine::Device;
use relocade_vga::Vga;
use std::rc::Rc;

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
    let mut vga = Vga::with_clock(Rc::new(ManualClock::default()));
    font_access(&mut vga);
    vga.write_mmio(0xae821, 1, 0xa5); // A glyph row in the last font bank.
    indexed(&mut vga, 0x3c4, 3, 0x35);
    indexed(&mut vga, 0x3d4, 0x11, 0); // Unlock geometry registers.
    indexed(&mut vga, 0x3d4, 1, 39);
    indexed(&mut vga, 0x3d4, 9, 15);
    indexed(&mut vga, 0x3d4, 0x13, 20);
    vga.write_port(0x3c0, 1, 0x12);
    vga.write_port(0x3c0, 1, 5);
    vga.write_port(0x3c0, 1, 0x34); // Keep the raw index together with display enable.
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
    let registers = snapshot.registers;
    assert_eq!(registers.sequencer.values[3], 0x35);
    assert_eq!(registers.crtc.values[1], 39);
    assert_eq!(registers.crtc.values[9], 15);
    assert_eq!(registers.crtc.values[0x13], 20);
    assert_eq!(registers.attribute.values[0x12], 5);
    assert_eq!(registers.sequencer.index(), 3);
    assert_eq!(registers.crtc.index(), 0x13);
    assert_eq!(registers.graphics.index(), 4);
    assert_eq!(registers.attribute.address(), 0x34);
    assert_eq!(registers.feature_control, 0x5a);
    assert_eq!(registers.dac.mask, 0x7f);
    assert_eq!(registers.dac.address(), 124);
    assert_eq!(registers.dac.colors[123], [63, 21, 7]);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn immediate_snapshots_preserve_cpu_latches_and_pending_port_transfers() {
    let mut vga = Vga::with_clock(Rc::new(ManualClock::default()));
    let source = vga.frame_source();
    font_access(&mut vga);
    vga.write_mmio(0xa0010, 1, 0x5a);
    vga.write_mmio(0xa0020, 1, 0xa5);
    assert_eq!(vga.read_mmio(0xa0010, 1), 0x5a);
    vga.write_port(0x3c0, 1, 4); // Pending Attribute data write.
    vga.write_port(0x3c8, 1, 42);
    vga.write_port(0x3c9, 1, 11); // Pending green and blue components.
    let snapshot = vga.snapshot();
    assert!(source.current_capture().is_none());
    assert_eq!(snapshot.plane(2)[0x20], 0xa5);
    vga.write_port(0x3c0, 1, 0x31);
    assert_eq!(vga.read_port(0x3c1, 1), 0x31);
    vga.write_port(0x3c9, 1, 22);
    vga.write_port(0x3c9, 1, 33);
    assert_eq!(vga.snapshot().registers.dac.colors[42], [11, 22, 33]);
    assert_eq!(snapshot.registers.attribute.address(), 4);
    assert_eq!(snapshot.registers.dac.address(), 42);
    indexed(&mut vga, 0x3ce, 5, 1); // Copy the original read latch.
    vga.write_mmio(0xa0020, 1, 0);
    assert_eq!(vga.read_mmio(0xa0020, 1), 0x5a);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn newest_capture_contains_changes_from_skipped_frames() {
    let clock = Rc::new(ManualClock::default());
    let mut vga = Vga::with_clock(clock.clone());
    let source = vga.frame_source();
    font_access(&mut vga);
    vga.write_mmio(0xa0100, 1, 0x12);
    clock.set(boundary(1));
    vga.read_port(0x3da, 1);
    let first = source.current_capture().unwrap();
    vga.write_mmio(0xa0200, 1, 0x34);
    clock.set(boundary(2));
    vga.read_port(0x3da, 1);
    let second = source.clone().current_capture().unwrap(); // An independent reader.
    vga.write_mmio(0xa0300, 1, 0x56);
    clock.set(boundary(3));
    vga.read_port(0x3da, 1);
    let newest = source.current_capture().unwrap();
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
    let mut vga = Vga::with_clock(Rc::new(ManualClock::default()));
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
