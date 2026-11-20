use super::{device, get, set};
use relocade_vga::DispiState;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn linear_framebuffer_shares_vram_with_the_banked_window_across_bank_and_viewport_changes() {
    let vga = device();
    for (index, value) in [(1, 640), (2, 480), (3, 24), (4, 0x41)] {
        set(&vga.io, index, value);
    }
    assert_eq!(get(&vga.io, 4), 0x41);
    vga.io.write_mmio(0xe000fffd, 8, 0x1122334455667788);
    assert_eq!(vga.io.read_mmio(0xafffd, 3), 0x667788);
    set(&vga.io, 5, 1);
    assert_eq!(vga.io.read_mmio(0xa0000, 5), 0x1122334455);
    let retained = vga.snapshot();
    vga.io.write_mmio(0xa0000, 2, 0xbbaa);
    assert_eq!(vga.io.read_mmio(0xe000fffd, 8), 0x112233bbaa667788);
    assert_eq!(retained.vram_byte(65536), Some(0x55));
    assert_eq!(vga.snapshot().vram_byte(65536), Some(0xaa));

    for (index, value) in [(6, 800), (8, 160), (9, 1), (5, 63)] {
        set(&vga.io, index, value);
    }
    assert_eq!(vga.io.read_mmio(0xe000fffd, 8), 0x112233bbaa667788);
    vga.io.write_mmio(0xafffe, 2, 0xcdef);
    assert_eq!(vga.io.read_mmio(0xe03ffffe, 4), 0xffffcdef);
    vga.io.write_mmio(0xe03fffff, 4, 0x123456ab);
    assert_eq!(vga.io.read_mmio(0xafffe, 2), 0xabef);
    assert_eq!(vga.io.read_mmio(0xe0000000, 4), 0);
    vga.io.write_mmio(0xdfffffff, 4, 0x88776655);
    assert_eq!(vga.io.read_mmio(0xe0000000, 4), 0x887766);
    assert_eq!(vga.io.read_mmio(0xdfffffff, 4), 0x887766ff);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn linear_framebuffer_follows_mode_decoding_and_restores_without_clearing_vram() {
    let vga = device();
    assert_eq!(vga.io.read_mmio(0xe0000042, 1), 0xff);
    for (index, value) in [(1, 640), (2, 480), (3, 8), (4, 0x41)] {
        set(&vga.io, index, value);
    }
    vga.io.write_mmio(0xe0000042, 1, 0xa5);
    let bytes = vga.device.borrow().dispi_state().to_bytes();
    assert_eq!(&bytes[8..10], &[0x41, 0]);
    let state = DispiState::from_bytes(&bytes).unwrap();
    for (port, disabled, enabled) in [(0x3c2, 0x41, 0x43), (0x46e8, 0, 0x0e)] {
        let core = vga.device.borrow().core_state();
        vga.io.write_port(port, 1, disabled);
        vga.io.write_mmio(0xe0000042, 1, 0);
        assert_eq!(vga.io.read_mmio(0xe0000042, 1), 0xff);
        vga.io.with_context(|context| {
            vga.device
                .borrow_mut()
                .restore_state(context, Some(core), None, None);
        });
        assert_eq!(vga.io.read_mmio(0xe0000042, 1), 0xa5);
        vga.io.write_port(port, 1, enabled);
    }
    set(&vga.io, 4, 0);
    vga.io.write_mmio(0xe0000042, 1, 0);
    assert_eq!(vga.io.read_mmio(0xe0000042, 1), 0xff);
    vga.io.with_context(|context| {
        vga.device
            .borrow_mut()
            .restore_state(context, None, None, Some(state));
    });
    assert_eq!(get(&vga.io, 4), 0x41);
    assert_eq!(vga.io.read_mmio(0xe0000042, 1), 0xa5);

    set(&vga.io, 4, 0);
    set(&vga.io, 4, 0xc1);
    assert_eq!(get(&vga.io, 4), 0xc1);
    assert_eq!(vga.io.read_mmio(0xe0000042, 1), 0xa5);
    set(&vga.io, 4, 0);
    set(&vga.io, 4, 1);
    // A banked packed mode also exposes the linear framebuffer with the LFB flag clear.
    assert_eq!(vga.io.read_mmio(0xe0000042, 1), 0);
    vga.io.write_mmio(0xe0000042, 1, 0x5a);
    assert_eq!(vga.io.read_mmio(0xa0042, 1), 0x5a);
}
