mod common;
use common::ManualClock;
use relocade_machine::Device;
use relocade_vga::Vga;
use std::rc::Rc;

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

fn set(vga: &mut impl Device, port: u16, index: u8, value: u8) {
    vga.write_port(port, 2, u32::from(u16::from_le_bytes([index, value])));
}

fn planar() -> Vga<ManualClock> {
    let mut vga = Vga::with_clock(Rc::new(ManualClock::default()));
    set(&mut vga, 0x3c4, 4, 6); // Extended memory; disable odd/even writes.
    set(&mut vga, 0x3c4, 2, 15);
    set(&mut vga, 0x3ce, 5, 0);
    set(&mut vga, 0x3ce, 6, 5); // A0000, 64 KiB, graphics addressing.
    vga
}

fn seed(vga: &mut impl Device, address: u32, bytes: [u8; 4]) {
    for (plane, byte) in bytes.into_iter().enumerate() {
        set(vga, 0x3c4, 2, 1 << plane);
        vga.write_mmio(address, 1, u64::from(byte));
    }
    set(vga, 0x3c4, 2, 15);
}

fn planes(vga: &mut impl Device, address: u32) -> [u8; 4] {
    std::array::from_fn(|plane| {
        set(vga, 0x3ce, 4, plane as u8);
        vga.read_mmio(address, 1) as u8
    })
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn write_modes_use_read_latches_instead_of_destination_bytes() {
    let mut vga = planar();
    seed(&mut vga, 0xa0010, [0xa5, 0x5a, 0xf0, 0x0f]);
    seed(&mut vga, 0xa0020, [0x11, 0x22, 0x33, 0x44]);
    set(&mut vga, 0x3ce, 4, 2);
    assert_eq!(vga.read_mmio(0xa0010, 1), 0xf0);
    set(&mut vga, 0x3c4, 2, 5);
    set(&mut vga, 0x3ce, 5, 1); // Write mode 1 copies all latch bits, ignoring host data.
    set(&mut vga, 0x3ce, 8, 0);
    vga.write_mmio(0xa0020, 1, 0);
    assert_eq!(planes(&mut vga, 0xa0020), [0xa5, 0x22, 0xf0, 0x44]);

    vga.read_mmio(0xa0010, 1);
    set(&mut vga, 0x3c4, 2, 15);
    set(&mut vga, 0x3ce, 5, 0);
    set(&mut vga, 0x3ce, 0, 5);
    set(&mut vga, 0x3ce, 1, 3);
    set(&mut vga, 0x3ce, 3, 0x19); // Rotate right once, XOR with latches.
    set(&mut vga, 0x3ce, 8, 0xf0);
    vga.write_mmio(0xa0030, 1, 0x80);
    assert_eq!(planes(&mut vga, 0xa0030), [0x55, 0x5a, 0xb0, 0x4f]);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn color_expansion_and_mode_three_apply_logic_and_pixel_masks() {
    let mut vga = planar();
    seed(&mut vga, 0xa0010, [0xa5, 0x5a, 0xf0, 0x0f]);
    vga.read_mmio(0xa0010, 1);
    set(&mut vga, 0x3ce, 5, 2);
    set(&mut vga, 0x3ce, 3, 0x0b); // AND; rotation does not affect color expansion.
    set(&mut vga, 0x3ce, 8, 0x3c);
    vga.write_mmio(0xa0020, 1, 5);
    assert_eq!(planes(&mut vga, 0xa0020), [0xa5, 0x42, 0xf0, 3]);
    vga.read_mmio(0xa0010, 1);
    set(&mut vga, 0x3ce, 5, 3);
    set(&mut vga, 0x3ce, 0, 10);
    set(&mut vga, 0x3ce, 1, 0); // Mode 3 ignores Enable Set/Reset.
    set(&mut vga, 0x3ce, 3, 0x14); // OR; rotate data to form the pixel mask.
    vga.write_mmio(0xa0030, 1, 0xf0);
    assert_eq!(planes(&mut vga, 0xa0030), [0xa5, 0x5e, 0xf0, 0x0f]);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn color_compare_ignores_unselected_planes_and_still_loads_every_latch() {
    let mut vga = planar();
    seed(&mut vga, 0xa0010, [0xa5, 0x5a, 0xf0, 0x0f]);
    set(&mut vga, 0x3ce, 2, 1);
    set(&mut vga, 0x3ce, 7, 3);
    set(&mut vga, 0x3ce, 5, 8);
    assert_eq!(vga.read_mmio(0xa0010, 1), 0xa5);
    set(&mut vga, 0x3ce, 7, 15);
    assert_eq!(vga.read_mmio(0xa0010, 1), 0);
    set(&mut vga, 0x3ce, 7, 0);
    assert_eq!(vga.read_mmio(0xa0010, 1), 0xff);
    set(&mut vga, 0x3ce, 5, 1);
    vga.write_mmio(0xa0020, 1, 0);
    assert_eq!(planes(&mut vga, 0xa0020), [0xa5, 0x5a, 0xf0, 0x0f]);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn chain_four_and_odd_even_access_the_same_physical_planes() {
    let mut vga = Vga::with_clock(Rc::new(ManualClock::default()));
    set(&mut vga, 0x3c4, 2, 15);
    vga.write_mmio(0xb8000, 4, 0x2f42_0741);
    assert_eq!(vga.read_mmio(0xb8000, 4), 0x2f42_0741);
    set(&mut vga, 0x3c4, 4, 6);
    set(&mut vga, 0x3ce, 5, 0);
    set(&mut vga, 0x3ce, 6, 5);
    assert_eq!(planes(&mut vga, 0xa0000), [0x41, 7, 0x41, 7]);
    assert_eq!(planes(&mut vga, 0xa0001), [0; 4]);
    assert_eq!(planes(&mut vga, 0xa0002), [0x42, 0x2f, 0x42, 0x2f]);
    set(&mut vga, 0x3c4, 4, 14);
    vga.write_mmio(0xa0000, 8, 0x0706_0504_0302_0100);
    assert_eq!(vga.read_mmio(0xa0000, 8), 0x0706_0504_0302_0100);
    set(&mut vga, 0x3c4, 2, 1);
    vga.write_mmio(0xa0004, 2, 0xbbaa); // Plane 1 is disabled.
    set(&mut vga, 0x3c4, 4, 6);
    assert_eq!(planes(&mut vga, 0xa0000), [0, 1, 2, 3]);
    assert_eq!(planes(&mut vga, 0xa0001), [0; 4]);
    assert_eq!(planes(&mut vga, 0xa0004), [0xaa, 5, 6, 7]);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn chain_four_routes_high_address_bits_to_the_plane_address() {
    let mut vga = planar();
    set(&mut vga, 0x3c4, 4, 14);
    // CPU A14/A15 replace the plane address's two low bits. These are the
    // same physical locations reached by the CRTC's doubleword rotation.
    for (cpu, physical) in [
        (0x3ffc, 0x3ffc),
        (0x4000, 0x4001),
        (0x7ffc, 0x7ffd),
        (0x8000, 0x8002),
        (0xbffc, 0xbffe),
        (0xc000, 0xc003),
        (0xfffc, 0xffff),
    ] {
        set(&mut vga, 0x3c4, 4, 14);
        vga.write_mmio(0xa0000 + cpu, 4, 0x44332211);
        assert_eq!(vga.read_mmio(0xa0000 + cpu, 4), 0x44332211);
        set(&mut vga, 0x3c4, 4, 6);
        assert_eq!(
            planes(&mut vga, 0xa0000 + physical),
            [0x11, 0x22, 0x33, 0x44]
        );
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn aperture_selection_bounds_cpu_transfers_and_planar_addresses_wrap() {
    let mut vga = planar();
    vga.write_mmio(0xa0000, 1, 0x5a);
    assert_eq!(vga.read_mmio(0xb0000, 1), 0xff);
    vga.write_mmio(0xaffff, 2, 0x1234);
    assert_eq!(vga.read_mmio(0xaffff, 2), 0xff34);
    set(&mut vga, 0x3ce, 6, 1); // Full 128-KiB aperture, 16-bit plane address.
    assert_eq!(vga.read_mmio(0xb0000, 1), 0x5a);
    for (setting, base) in [(9, 0xb0000), (13, 0xb8000)] {
        set(&mut vga, 0x3ce, 6, setting);
        assert_eq!(vga.read_mmio(base, 1), 0x5a);
        assert_eq!(vga.read_mmio(base - 1, 1), 0xff);
        assert_eq!(vga.read_mmio(base + 0x8000, 1), 0xff);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn wide_reads_leave_the_last_latches_and_disabled_reads_preserve_them() {
    let mut vga = planar();
    seed(&mut vga, 0xa0010, [0xa5, 0x5a, 0xf0, 0x0f]);
    seed(&mut vga, 0xa0011, [0x3c, 0xc3, 0x96, 0x69]);
    assert_eq!(vga.read_mmio(0xa0010, 2), 0x3ca5);
    assert_eq!(vga.read_mmio(0xb0000, 1), 0xff); // Inactive aperture.
    vga.write_port(0x3c2, 1, 0x41); // Disable video RAM decoding.
    assert_eq!(vga.read_mmio(0xa0010, 1), 0xff);
    vga.write_mmio(0xa0011, 1, 0);
    vga.write_port(0x3c2, 1, 0x43);
    set(&mut vga, 0x3ce, 5, 1);
    vga.write_mmio(0xa0020, 2, 0);
    assert_eq!(planes(&mut vga, 0xa0020), [0x3c, 0xc3, 0x96, 0x69]);
    assert_eq!(planes(&mut vga, 0xa0021), [0x3c, 0xc3, 0x96, 0x69]);
    assert_eq!(planes(&mut vga, 0xa0011), [0x3c, 0xc3, 0x96, 0x69]);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn address_chaining_is_independent_of_read_and_write_plane_selection() {
    let mut vga = planar();
    seed(&mut vga, 0xa0000, [0xaa, 0xbb, 0xcc, 0xdd]);
    seed(&mut vga, 0xa0001, [0x11, 0x22, 0x33, 0x44]);
    set(&mut vga, 0x3ce, 4, 2);
    set(&mut vga, 0x3ce, 5, 0x10); // Odd/even reads, but no address-bit replacement.
    assert_eq!(vga.read_mmio(0xa0000, 1), 0xcc);
    assert_eq!(vga.read_mmio(0xa0001, 1), 0x44);
    set(&mut vga, 0x3ce, 6, 7); // Address-bit replacement now aliases these offsets.
    assert_eq!(vga.read_mmio(0xa0001, 1), 0xdd);

    set(&mut vga, 0x3ce, 5, 0);
    set(&mut vga, 0x3ce, 6, 5);
    set(&mut vga, 0x3c4, 4, 2); // Odd/even writes still leave the physical offset intact.
    vga.write_mmio(0xa0001, 1, 5);
    assert_eq!(planes(&mut vga, 0xa0001), [0x11, 5, 0x33, 5]);
    set(&mut vga, 0x3c4, 4, 6);
    set(&mut vga, 0x3ce, 4, 0);
    set(&mut vga, 0x3ce, 6, 3); // The 128-KiB aperture routes A16 to address bit zero.
    assert_eq!(vga.read_mmio(0xb0000, 1), 0x11);
    assert_eq!(vga.read_mmio(0xa0000, 1), 0xaa);
    set(&mut vga, 0x3ce, 6, 1);
    set(&mut vga, 0x3c4, 4, 4); // Disable extended memory: 16 KiB per plane.
    assert_eq!(vga.read_mmio(0xa4000, 1), 0xaa);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn display_addressing_preserves_high_counter_bits_in_word_and_doubleword_modes() {
    let mut vga = planar();
    for (address, character) in [
        (0xad001, b'A'),
        (0xaa003, b'B'),
        (0xaa002, b'C'),
        (0xa4005, b'D'),
        (0xa9001, b'E'),
    ] {
        seed(&mut vga, address, [character, 7, 0, 0]);
    }
    set(&mut vga, 0x3d4, 0x0c, 0xd0);
    set(&mut vga, 0x3d4, 0x0d, 1);
    // Counter D001: byte addressing, word rotation from bit 15 or 13,
    // doubleword rotation from bits 12–13, and row-scan substitution for bit 14.
    for (mode, underline, character) in [
        (0xe3, 0, b'A'),
        (0xa3, 0, b'B'),
        (0x83, 0, b'C'),
        (0xe3, 0x40, b'D'),
        (0xe1, 0, b'E'),
    ] {
        set(&mut vga, 0x3d4, 0x17, mode);
        set(&mut vga, 0x3d4, 0x14, underline);
        assert_eq!(
            common::support::inspection::inspect(&vga.snapshot()).cells[0].character,
            character
        );
    }
}
