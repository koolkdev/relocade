mod common;
use common::ManualClock;
use relocade_machine::Device;
use relocade_vga::Vga;
use std::rc::Rc;

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn attribute_access_alternates_writes_and_only_active_status_resets_it() {
    let mut vga = Vga::with_clock(Rc::new(ManualClock::default()));
    vga.write_port(0x3c0, 1, 2);
    assert_eq!(vga.read_port(0x3c0, 1) & 0x3f, 2);
    assert_eq!(vga.read_port(0x3c1, 1) & 0x3f, 2);
    vga.read_port(0x3ba, 1); // Inactive status address must not reset the phase.
    vga.write_port(0x3c0, 1, 0x31);
    assert_eq!(vga.read_port(0x3c1, 1) & 0x3f, 0x31);
    vga.write_port(0x3c0, 1, 3);
    vga.read_port(0x3da, 1); // Abandon the pending data write.
    vga.write_port(0x3c0, 1, 4);
    vga.write_port(0x3c0, 1, 0xee);
    assert_eq!(vga.read_port(0x3c1, 1) & 0x3f, 0x2e);
    vga.write_port(0x3c0, 1, 0x24); // Palette access disabled during normal output.
    vga.write_port(0x3c0, 1, 0);
    vga.write_port(0x3c0, 1, 4);
    assert_eq!(vga.read_port(0x3c1, 1) & 0x3f, 0x2e);
    vga.write_port(0x3c2, 1, 0x42);
    vga.read_port(0x3ba, 1);
    vga.write_port(0x3c0, 1, 0x10);
    vga.write_port(0x3c0, 1, 0xf5);
    assert_eq!(vga.read_port(0x3c1, 1) & 0xef, 0xe5);
    vga.write_port(0x3c0, 1, 0x1f);
    vga.write_port(0x3c0, 1, 0xff); // Unsupported indices still consume a data write.
    assert_eq!(vga.read_port(0x3c1, 1), 0xff);
    vga.write_port(0x3c0, 1, 0x14);
    vga.write_port(0x3c0, 1, 9);
    assert_eq!(vga.read_port(0x3c1, 1) & 15, 9);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn dac_cycles_mask_channels_wrap_addresses_and_restart_partial_transfers() {
    let mut vga = Vga::with_clock(Rc::new(ManualClock::default()));
    vga.write_port(0x3c8, 1, 255);
    for value in [0xff, 0x21, 0x02, 0x03, 0x04, 0x05] {
        vga.write_port(0x3c9, 1, value);
    }
    assert_eq!(vga.read_port(0x3c8, 1), 1);
    assert_eq!(vga.read_port(0x3c7, 1) & 3, 0);
    vga.write_port(0x3c7, 1, 255);
    assert_eq!(vga.read_port(0x3c7, 1) & 3, 3);
    for expected in [0x3f, 0x21, 2, 3, 4, 5] {
        assert_eq!(vga.read_port(0x3c9, 1) & 0x3f, expected);
    }
    vga.write_port(0x3c8, 1, 2);
    vga.write_port(0x3c9, 1, 10);
    vga.write_port(0x3c8, 1, 3);
    for value in [11, 12, 13] {
        vga.write_port(0x3c9, 1, value);
    }
    vga.write_port(0x3c7, 1, 3);
    assert_eq!(vga.read_port(0x3c9, 1) & 0x3f, 11);
    vga.write_port(0x3c7, 1, 3); // Restart an incomplete RGB read at red.
    for expected in [11, 12, 13] {
        assert_eq!(vga.read_port(0x3c7, 1) & 3, 3);
        vga.read_port(0x3c8, 1); // Address/state reads do not advance the RGB phase.
        assert_eq!(vga.read_port(0x3c9, 1) & 0x3f, expected);
    }
    vga.write_port(0x3c6, 1, 0x55);
    assert_eq!(vga.read_port(0x3c6, 1), 0x55);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn graphics_and_remaining_crtc_registers_read_back_the_defined_fields() {
    let mut vga = Vga::with_clock(Rc::new(ManualClock::default()));
    for (index, mask, expected) in [
        (0, 0x0f, 5),
        (1, 0x0f, 5),
        (2, 0x0f, 5),
        (3, 0x1f, 5),
        (4, 0x03, 1),
        (5, 0x7b, 0x21),
        (6, 0x0f, 5),
        (7, 0x0f, 5),
        (8, 0xff, 0xa5),
    ] {
        vga.write_port(0x3ce, 2, 0xa500 | index);
        assert_eq!(vga.read_port(0x3cf, 1) & mask, expected);
    }
    vga.write_port(0x3ce, 1, 0x1f);
    assert_eq!(vga.read_port(0x3ce, 1) & 15, 15);
    assert_eq!(vga.read_port(0x3cf, 1), 255);
    for (index, mask, expected) in [
        (8, 0x7f, 0x25),
        (0x13, 0xff, 0xa5),
        (0x14, 0x7f, 0x25),
        (0x18, 0xff, 0xa5),
    ] {
        vga.write_port(0x3d4, 2, 0xa500 | index);
        assert_eq!(vga.read_port(0x3d5, 1) & mask, expected);
    }
}
