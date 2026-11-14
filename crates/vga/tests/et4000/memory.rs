use super::*;

fn configure(io: &IoBus, chain4: bool) {
    io.write_port(0x3bf, 1, 3);
    io.write_port(0x3d8, 1, 0xa0);
    for value in [0x0001, 0x0003, 0x0005, 0x0506, 0xff08] {
        io.write_port(0x3ce, 2, value);
    }
    io.write_port(0x3c4, 2, if chain4 { 0x0e04 } else { 0x0604 });
    io.write_port(0x3c4, 2, 0x0f02);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn legacy_indexed_scanout_reads_the_bytes_written_through_the_cpu_aperture() {
    let device = ConnectedEt4000::new(&ManualClock::default());
    support::indexed256::program(&device.io);
    for (offset, value) in [(0, 0x11), (4, 0x22), (320, 0x33), (320 * 199 + 319, 0x44)] {
        device.io.write_mmio(0xa0000 + offset, 1, value);
    }
    let mut card = device.card.borrow_mut();
    assert_eq!(card.snapshot().geometry().size, [320, 200]);
    let framebuffer = card.framebuffer();
    for (x, y, value) in [(0, 0, 0x11), (4, 0, 0x22), (0, 1, 0x33), (319, 199, 0x44)] {
        assert_eq!(framebuffer.pixel(x, y).unwrap().value(), value);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn linear_banks_are_independent_and_use_all_installed_memory() {
    let device = ConnectedEt4000::new(&ManualClock::default());
    let io = &device.io;
    configure(io, true);
    // Linear-byte addressing ignores both VGA plane selection registers.
    io.write_port(0x3c4, 2, 0x0002);
    io.write_port(0x3ce, 2, 0x0304);
    for bank in 0..16 {
        io.write_port(0x3cd, 1, bank);
        io.write_mmio(0xa0000, 4, 0x12345600 | u64::from(bank));
        io.write_mmio(0xaffff, 1, 0x80 | u64::from(bank));
    }
    for bank in 0..16 {
        io.write_port(0x3cd, 1, (bank << 4) | 7);
        assert_eq!(io.read_mmio(0xa0000, 4), 0x12345600 | u64::from(bank));
        assert_eq!(io.read_mmio(0xaffff, 2), 0xff80 | u64::from(bank));
    }
    io.write_port(0x3cd, 1, 0xf0); // Read last bank, write bank zero.
    io.write_mmio(0xa0000, 1, 0x42);
    assert_eq!(io.read_mmio(0xa0000, 1), 15);
    io.write_port(0x3cd, 1, 0);
    assert_eq!(io.read_mmio(0xa0000, 1), 0x42);
    assert_eq!(
        device.card.borrow().snapshot().vga().unwrap().display_start,
        0
    );
    // Switching interpretations exposes the same bytes; no repacking occurs.
    io.write_port(0x3c4, 2, 0x0604);
    io.write_port(0x3cd, 1, 0x30);
    io.write_port(0x3ce, 2, 0x0304);
    assert_eq!(io.read_mmio(0xaffff, 1), 0x8f);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn banked_planar_reads_load_latches_for_writes_to_another_bank() {
    let device = ConnectedEt4000::new(&ManualClock::default());
    let io = &device.io;
    configure(io, false);
    io.write_port(0x3cd, 1, 1);
    for (plane, value) in [0x12, 0x34, 0x56, 0x78].into_iter().enumerate() {
        io.write_port(0x3c4, 2, (0x100 << plane) | 2);
        io.write_mmio(0xa1234, 1, value);
    }
    io.write_port(0x3cd, 1, 0x13);
    io.write_port(0x3ce, 2, 0x0204);
    assert_eq!(io.read_mmio(0xa1234, 1), 0x56);
    io.write_port(0x3c4, 2, 0x0f02);
    io.write_port(0x3ce, 2, 0x0105); // Write mode one copies all read latches.
    io.write_mmio(0xa5678, 1, 0xff);
    io.write_port(0x3cd, 1, 0xf0); // Higher plane-bank bits wrap at installed capacity.
    for (plane, value) in [0x12, 0x34, 0x56, 0x78].into_iter().enumerate() {
        io.write_port(0x3ce, 2, ((plane as u32) << 8) | 4);
        assert_eq!(io.read_mmio(0xa5678, 1), value);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn aperture_edges_follow_the_live_mapping_and_linear_a16_is_ignored() {
    let device = ConnectedEt4000::new(&ManualClock::default());
    let io = &device.io;
    configure(io, true);
    io.write_port(0x3cd, 1, 0xff);
    io.write_mmio(0xafffe, 4, 0x44332211);
    assert_eq!(io.read_mmio(0xafffe, 4), 0xffff2211);
    io.write_port(0x3ce, 2, 0x0106); // The VGA 128-KiB window mirrors A16.
    io.write_mmio(0xb0000, 1, 0x66);
    assert_eq!(io.read_mmio(0xa0000, 1), 0x66);
    assert_eq!(io.read_mmio(0xaffff, 2), 0x6622);
    io.write_port(0x3ce, 2, 0x0d06); // B8000–BFFFF window.
    assert_eq!(io.read_mmio(0xa0000, 1), 0xff);
    io.write_mmio(0xb8000, 1, 0x77);
    assert_eq!(io.read_mmio(0xb8000, 1), 0x77);
    io.write_port(0x46e8, 1, 0);
    io.write_mmio(0xb8000, 1, 0x88);
    io.write_port(0x46e8, 1, 8);
    assert_eq!(io.read_mmio(0xb8000, 1), 0x77);
}
