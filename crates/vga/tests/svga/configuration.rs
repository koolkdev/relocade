use super::{device, get, set};
use relocade_vga::{DispiMode, DispiResolution, PixelFormat};

const MODE: DispiMode = DispiMode::new(DispiResolution::R640x480, PixelFormat::Indexed8);

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn direct_mode_entry_preserves_or_clears_memory_and_leaves_the_enable_index() {
    let card = device();
    assert!(card.io.with_context(|context| card
        .device
        .borrow_mut()
        .enter_packed_mode(context, MODE, false)));
    set(&card.io, 5, 7);
    card.io.write_mmio(0xa0000, 2, 0xabcd);
    assert!(card.io.with_context(|context| card
        .device
        .borrow_mut()
        .enter_packed_mode(context, MODE, true)));
    assert_eq!(card.io.read_port(0x1ce, 2), 4);
    assert_eq!(get(&card.io, 4), 0x81);
    assert_eq!(get(&card.io, 5), 0);
    assert!(card
        .io
        .with_context(|context| card.device.borrow_mut().set_video_bank(context, 7)));
    assert_eq!(card.io.read_port(0x1ce, 2), 5);
    assert_eq!(card.io.read_mmio(0xa0000, 2), 0xabcd);
    assert!(card.io.with_context(|context| card
        .device
        .borrow_mut()
        .enter_packed_mode(context, MODE, false)));
    set(&card.io, 5, 7);
    assert_eq!(card.io.read_mmio(0xa0000, 2), 0);
    card.io
        .with_context(|context| card.device.borrow_mut().prepare_legacy_mode(context));
    assert_eq!(card.io.read_port(0x1ce, 2), 4);
    assert_eq!(get(&card.io, 4), 0);
    assert_eq!(card.io.read_mmio(0xb8000, 2), 0);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn direct_layout_programming_preserves_indices_and_rejects_invalid_viewports() {
    let card = device();
    assert!(card.io.with_context(|context| card
        .device
        .borrow_mut()
        .enter_packed_mode(context, MODE, false)));
    assert!(card
        .io
        .with_context(|context| card.device.borrow_mut().set_packed_pitch(context, 2048))
        .is_some());
    assert_eq!(card.io.read_port(0x1ce, 2), 6);
    assert!(card.io.with_context(|context| card
        .device
        .borrow_mut()
        .set_display_origin(context, [1024, 0])));
    assert_eq!(card.io.read_port(0x1ce, 2), 9);
    assert_eq!(get(&card.io, 8), 1024);
    let before = card.device.borrow().dispi_state();
    card.io.with_context(|mut context| {
        let mut device = card.device.borrow_mut();
        assert!(device.set_packed_pitch(context.reborrow(), 1024).is_none());
        assert!(!device.set_display_origin(context.reborrow(), [1409, 0]));
        assert!(!device.set_video_bank(context, 64));
    });
    assert_eq!(card.device.borrow().dispi_state(), before);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn disabled_decoding_blocks_direct_firmware_programming() {
    let card = device();
    assert!(card.io.with_context(|context| card
        .device
        .borrow_mut()
        .enter_packed_mode(context, MODE, false)));
    card.io.write_mmio(0xa0000, 2, 0xabcd);
    card.io.write_port(0x1ce, 2, 0x104);
    card.io.write_port(0x46e8, 1, 0);
    let before = card.device.borrow().dispi_state();
    card.io.with_context(|mut context| {
        let mut device = card.device.borrow_mut();
        assert!(!device.ports_enabled());
        device.prepare_legacy_mode(context.reborrow());
        assert!(!device.enter_packed_mode(context.reborrow(), MODE, false));
        assert!(!device.set_video_bank(context.reborrow(), 7));
        assert!(device.set_packed_pitch(context.reborrow(), 2048).is_none());
        assert!(!device.set_display_origin(context, [0, 1]));
    });
    assert_eq!(card.device.borrow().dispi_state(), before);
    card.io.write_port(0x46e8, 1, 0x0e);
    assert_eq!(card.io.read_port(0x1ce, 2), 0x104);
    assert_eq!(card.io.read_mmio(0xa0000, 2), 0xabcd);
}
