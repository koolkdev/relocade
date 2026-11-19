use super::{device, enable, get, set};
use relocade_vga::{DacState, DispiState};

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn capability_queries_preserve_staged_and_active_programming() {
    let vga = device();
    for (index, value) in [(1, 640), (2, 480), (3, 8)] {
        set(&vga.io, index, value);
    }
    for flags in [0, 1] {
        set(&vga.io, 4, flags);
        set(&vga.io, 5, 7);
        if flags != 0 {
            vga.io.write_mmio(0xa0000, 1, 0xa5);
        }
        let layout = vga.device.borrow().packed_layout();
        set(&vga.io, 4, flags | 2);
        assert_eq!(get(&vga.io, 4), u32::from(flags | 2));
        assert_eq!(
            [get(&vga.io, 1), get(&vga.io, 2), get(&vga.io, 3)],
            [1024, 768, 32]
        );
        assert_eq!(vga.device.borrow().packed_layout(), layout);
        assert_eq!(get(&vga.io, 5), 7);
        set(&vga.io, 4, flags);
        assert_eq!(
            [get(&vga.io, 1), get(&vga.io, 2), get(&vga.io, 3)],
            [640, 480, 8]
        );
        if flags != 0 {
            assert_eq!(vga.io.read_mmio(0xa0000, 1), 0xa5);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn dac_precision_changes_share_ports_and_preserve_layout_memory_and_access_phase() {
    let vga = device();
    enable(&vga.io);
    set(&vga.io, 6, 1024);
    set(&vga.io, 8, 8);
    set(&vga.io, 9, 4);
    set(&vga.io, 5, 7);
    vga.io.write_mmio(0xa0000, 1, 0xa5);
    vga.io.write_port(0x3c8, 1, 255);
    vga.io.write_port(0x3c9, 1, 63); // Leave the transfer at green.
    let before = vga.snapshot();
    set(&vga.io, 4, 0x21);
    vga.io.write_port(0x3c9, 1, 129);
    vga.io.write_port(0x3c9, 1, 255);
    assert_eq!(vga.snapshot().dac.colors[255], [252, 129, 255]);
    assert_eq!(vga.snapshot().dac.bits_per_component, 8);
    assert_eq!(before.dac.bits_per_component, 6);
    assert_eq!(before.dac.colors[255][0], 63);
    assert_eq!(vga.snapshot().scanout(), before.scanout());
    assert_eq!(get(&vga.io, 5), 7);
    assert_eq!(vga.io.read_mmio(0xa0000, 1), 0xa5);

    vga.io.write_port(0x3c7, 1, 255);
    assert_eq!(vga.io.read_port(0x3c9, 1), 252);
    vga.io.with_context(|context| {
        vga.device
            .borrow_mut()
            .set_dac_bits_per_component(context, 6)
    });
    assert_eq!(get(&vga.io, 4), 1);
    assert_eq!(vga.io.read_port(0x3c9, 1), 32);
    let state = vga.device.borrow().dac_state();
    set(&vga.io, 4, 0x25); // A reserved flag rejects the whole write.
    assert_eq!(vga.device.borrow().dac_state(), state);
    set(&vga.io, 4, 0x20); // DAC precision also applies in legacy VGA mode.
    assert!(vga.device.borrow().packed_layout().is_none());
    assert_eq!(get(&vga.io, 4), 0x20);
    assert_eq!(vga.device.borrow().dac_bits_per_component(), 8);
    vga.io
        .with_context(|context| vga.device.borrow_mut().prepare_legacy_mode(context));
    assert_eq!(vga.device.borrow().dac_bits_per_component(), 6);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn dac_and_dispi_state_restore_independent_controls() {
    let vga = device();
    enable(&vga.io);
    set(&vga.io, 0, 0xb0c1);
    set(&vga.io, 4, 0x23);
    vga.io.write_port(0x3c8, 1, 17);
    vga.io.write_port(0x3c9, 1, 200);
    let dac = DacState::from_bytes(&vga.device.borrow().dac_state().to_bytes()).unwrap();
    let bytes = vga.device.borrow().dispi_state().to_bytes();
    assert_eq!(&bytes[8..10], &[3, 0]); // Precision is owned by the DAC state class.
    assert_eq!(&bytes[18..20], &[0xc1, 0xb0]);
    let dispi = DispiState::from_bytes(&bytes).unwrap();
    set(&vga.io, 0, 0xb0c3);
    set(&vga.io, 4, 1);
    let six = vga.device.borrow().dac_state();
    vga.io.with_context(|context| {
        vga.device
            .borrow_mut()
            .restore_state(context, None, None, Some(dispi))
    });
    assert_eq!(get(&vga.io, 0), 0xb0c1);
    assert_eq!(get(&vga.io, 4), 3);
    assert_eq!(vga.device.borrow().dac_state(), six);
    let mode = vga.device.borrow().dispi_state();
    vga.io.with_context(|context| {
        vga.device
            .borrow_mut()
            .restore_state(context, None, Some(dac), None)
    });
    assert_eq!(get(&vga.io, 4), 0x23);
    // Port index reads change the selected index; the underlying mode is intact.
    assert_eq!(vga.device.borrow().dispi_state(), mode);
    vga.io.write_port(0x3c9, 1, 129);
    vga.io.write_port(0x3c9, 1, 255);
    assert_eq!(vga.snapshot().dac.colors[17], [200, 129, 255]);
}
