mod common;
#[path = "svga/frames.rs"]
mod frames;
#[path = "svga/layout.rs"]
mod layout;

use common::support::svga::{enable, get, set};
use common::ManualClock;
use relocade_machine::Device;
use relocade_vga::Vga;
use std::rc::Rc;

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

fn device() -> Vga<ManualClock> {
    Vga::with_clock(Rc::new(ManualClock::default()))
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn dispi_ports_accept_whole_words_and_keep_full_register_indices() {
    let mut vga = device();
    assert_eq!(get(&mut vga, 0), 0xb0c1);
    set(&mut vga, 0, 0xb0c5);
    assert_eq!(get(&mut vga, 0), 0xb0c1);
    set(&mut vga, 0x104, 1); // Must not alias the enable register.
    assert_eq!(vga.read_port(0x1ce, 2), 0x104);
    assert_eq!(vga.read_port(0x1cf, 2), 0xffff);
    for bytes in [1, 4] {
        vga.write_port(0x1ce, bytes, 4);
        vga.write_port(0x1cf, bytes, 1);
        assert_eq!(vga.read_port(0x1ce, 2), 0x104);
        let open = if bytes == 1 { 0xff } else { u32::MAX };
        assert_eq!(vga.read_port(0x1ce, bytes), open);
        assert_eq!(vga.read_port(0x1cf, bytes), open);
    }
    assert_eq!(get(&mut vga, 4), 0);
    assert_eq!(get(&mut vga, 0xffff), 0xffff);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn mode_enable_requires_supported_geometry_and_preserves_active_programming() {
    let mut vga = device();
    set(&mut vga, 4, 1);
    assert_eq!(get(&mut vga, 4), 0);
    set(&mut vga, 1, 641);
    set(&mut vga, 2, 480);
    set(&mut vga, 3, 8);
    set(&mut vga, 4, 1);
    assert_eq!(get(&mut vga, 1), 641);
    assert_eq!(get(&mut vga, 4), 0);
    set(&mut vga, 1, 640);
    set(&mut vga, 3, 16);
    set(&mut vga, 4, 1);
    assert_eq!(get(&mut vga, 4), 0);
    set(&mut vga, 3, 8);
    vga.write_mmio(0xb8000, 2, 0x0741);
    for flags in [0x41, 0xc1, 0x80, 0x02] {
        set(&mut vga, 4, flags);
        assert_eq!(get(&mut vga, 4), 0);
        assert_eq!(vga.read_mmio(0xb8000, 2), 0x0741);
    }
    enable(&mut vga);
    set(&mut vga, 5, 7);
    vga.write_mmio(0xa0000, 1, 0xa5);
    for (index, value) in [(1, 800), (2, 600), (3, 32), (4, 0x41), (4, 0x81), (4, 1)] {
        set(&mut vga, index, value);
    }
    assert_eq!(
        (get(&mut vga, 1), get(&mut vga, 2), get(&mut vga, 3)),
        (640, 480, 8)
    );
    assert_eq!(get(&mut vga, 4), 1);
    assert_eq!(get(&mut vga, 5), 7);
    assert_eq!(vga.read_mmio(0xa0000, 1), 0xa5);
    set(&mut vga, 4, 0);
    set(&mut vga, 4, 1);
    assert_eq!(get(&mut vga, 5), 0);
    set(&mut vga, 5, 7);
    assert_eq!(vga.read_mmio(0xa0000, 1), 0);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn no_clear_mode_entry_preserves_all_banks_and_resets_only_the_window() {
    let mut vga = device();
    enable(&mut vga);
    for bank in [0, 1, 63] {
        set(&mut vga, 5, bank);
        vga.write_mmio(0xa0000, 2, 0x1234 + u64::from(bank));
        vga.write_mmio(0xaffff, 1, 0xa5);
    }
    set(&mut vga, 4, 0);
    set(&mut vga, 4, 0x81);
    assert_eq!(get(&mut vga, 4), 0x81);
    assert_eq!(get(&mut vga, 5), 0);
    for bank in [0, 1, 63] {
        set(&mut vga, 5, bank);
        assert_eq!(vga.read_mmio(0xa0000, 2), 0x1234 + u64::from(bank));
        assert_eq!(vga.read_mmio(0xaffff, 1), 0xa5);
    }
    // Repeated enables and unsupported flag combinations leave the active mode alone.
    for flags in [1, 0x81, 0xc1, 0x80] {
        set(&mut vga, 4, flags);
        assert_eq!(get(&mut vga, 4), 0x81);
        assert_eq!(get(&mut vga, 5), 63);
        assert_eq!(vga.read_mmio(0xa0000, 2), 0x1273);
    }
    set(&mut vga, 4, 0);
    assert_eq!(get(&mut vga, 4), 0);
    set(&mut vga, 4, 1);
    for bank in [0, 1, 63] {
        set(&mut vga, 5, bank);
        assert_eq!(vga.read_mmio(0xa0000, 2), 0);
        assert_eq!(vga.read_mmio(0xaffff, 1), 0);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn bank_selection_reaches_all_vram_without_wrapping_invalid_banks() {
    let mut vga = device();
    enable(&mut vga);
    for (bank, value) in [(0, 0x1234), (1, 0x5678), (63, 0xabcd)] {
        set(&mut vga, 5, bank);
        vga.write_mmio(0xa0000, 2, value);
        vga.write_mmio(0xafff8, 8, value << 48);
    }
    for (bank, value) in [(0, 0x1234), (1, 0x5678), (63, 0xabcd)] {
        set(&mut vga, 5, bank);
        assert_eq!(vga.read_mmio(0xa0000, 2), value);
        assert_eq!(vga.read_mmio(0xafff8, 8), value << 48);
    }
    for bank in [64, 0xffff] {
        set(&mut vga, 5, bank);
        assert_eq!(get(&mut vga, 5), 63);
        assert_eq!(vga.read_mmio(0xafffe, 2), 0xabcd);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn packed_aperture_closes_legacy_windows_and_splits_boundary_accesses() {
    let mut vga = device();
    enable(&mut vga);
    set(&mut vga, 5, 63);
    vga.write_mmio(0xafffe, 4, 0x1234_beef);
    assert_eq!(vga.read_mmio(0xafffe, 4), 0xffff_beef);
    vga.write_mmio(0xb8000, 2, 0x0741);
    assert_eq!(vga.read_mmio(0xb8000, 2), 0xffff);
    vga.write_port(0x3c2, 1, 0x41); // Disable display-memory decoding.
    vga.write_mmio(0xafffe, 2, 0);
    assert_eq!(vga.read_mmio(0xafffe, 2), 0xffff);
    vga.write_port(0x3c2, 1, 0x43);
    assert_eq!(vga.read_mmio(0xafffe, 2), 0xbeef);
    vga.write_port(0x46e8, 1, 0); // Adapter address decoding gates DISPI too.
    for bytes in [1, 2, 4] {
        assert_eq!(vga.read_port(0x1ce, bytes), u32::MAX >> ((4 - bytes) * 8));
    }
    set(&mut vga, 4, 0);
    vga.write_mmio(0xafffe, 2, 0);
    assert_eq!(vga.read_mmio(0xafffe, 2), 0xffff);
    vga.write_port(0x46e8, 1, 0x0e);
    assert_eq!(get(&mut vga, 4), 1);
    assert_eq!(vga.read_mmio(0xafffe, 2), 0xbeef);
    set(&mut vga, 4, 0);
    assert_eq!(vga.read_mmio(0xb8000, 2), 0);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn packed_accesses_share_vga_planes_without_changing_read_latches() {
    let mut vga = device();
    enable(&mut vga);
    vga.write_mmio(0xa0000, 4, 0x4433_2211);
    let before = vga.snapshot();
    for (plane, value) in [0x11, 0x22, 0x33, 0x44].into_iter().enumerate() {
        assert_eq!(before.plane(plane)[0], value);
    }
    set(&mut vga, 4, 0);
    for (port, value) in [
        (0x3c4, 0x0604),
        (0x3c4, 0x0f02),
        (0x3ce, 0x0005),
        (0x3ce, 0x0506),
    ] {
        vga.write_port(port, 2, value);
    }
    assert_eq!(vga.read_mmio(0xa0000, 1), 0x11); // Latch all four original bytes.
    set(&mut vga, 4, 1);
    vga.write_mmio(0xa0000, 4, 0x8877_6655);
    assert_eq!(vga.read_mmio(0xa0000, 4), 0x8877_6655);
    set(&mut vga, 4, 0);
    vga.write_port(0x3ce, 2, 0x0105); // VGA write mode 1 copies read latches.
    vga.write_mmio(0xa0010, 1, 0);
    let after = vga.snapshot();
    for (plane, value) in [0x11, 0x22, 0x33, 0x44].into_iter().enumerate() {
        assert_eq!(after.plane(plane)[0x10], value);
        assert_eq!(before.plane(plane)[0], value);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn svga_uses_the_existing_dac_without_resetting_legacy_registers() {
    let mut vga = device();
    let registers = vga.registers().clone();
    enable(&mut vga);
    assert_eq!(vga.registers(), &registers);
    vga.write_port(0x3c8, 2, 0x012a); // Index 42 and red component through byte VGA ports.
    vga.write_port(0x3c9, 1, 2);
    vga.write_port(0x3c9, 1, 3);
    set(&mut vga, 4, 0);
    assert_eq!(vga.snapshot().dac.colors[42], [1, 2, 3]);
    assert_eq!(vga.registers().crtc, registers.crtc);
}
