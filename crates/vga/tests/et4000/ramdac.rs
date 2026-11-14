use super::{scanout, ConnectedEt4000, ManualClock};
use relocade_machine::IoBus;
use relocade_vga::{Et4000State, FramebufferAccess, PixelFormat};

pub(super) fn command(io: &IoBus, value: u8) {
    io.read_port(0x3c8, 1);
    for _ in 0..4 {
        io.read_port(0x3c6, 1);
    }
    io.write_port(0x3c6, 1, u32::from(value));
}

pub(super) fn rgb555_640(io: &IoBus) {
    scanout::indexed_640(io);
    // Word output, with the required MCLK/2, and a 1280-byte row.
    io.write_port(0x3c4, 2, 0xfc07);
    io.read_port(0x3da, 1);
    io.write_port(0x3c0, 1, 0x36);
    io.write_port(0x3c0, 1, 0x30);
    io.write_port(0x3d4, 2, 0xa013);
    command(io, 0x80);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn command_access_requires_four_reads_and_ends_on_writes_or_palette_access() {
    let device = ConnectedEt4000::new(&ManualClock::default());
    let io = &device.io;
    io.write_port(0x3c6, 1, 0x5a);
    for _ in 0..3 {
        assert_eq!(io.read_port(0x3c6, 1), 0x5a);
    }
    io.write_port(0x3c6, 1, 0x3f); // Too early: still writes the pixel mask.
    for _ in 0..4 {
        assert_eq!(io.read_port(0x3c6, 1), 0x3f);
    }
    assert_eq!(io.read_port(0x3c6, 1), 0); // Command readback.
    io.write_port(0x3c6, 1, 0xa0);
    assert_eq!(io.read_port(0x3c6, 1), 0x3f); // Command write consumed access.
    for _ in 0..3 {
        io.read_port(0x3c6, 1);
    }
    assert_eq!(io.read_port(0x3c6, 1), 0xa0);
    // A wide read executes the command read first, then the state read resets it.
    assert_eq!(io.read_port(0x3c6, 2) & 0xff, 0xa0);
    assert_eq!(io.read_port(0x3c6, 1), 0x3f);
    for port in [0x3c7, 0x3c8, 0x3c9] {
        command(io, 0x80);
        for _ in 0..4 {
            io.read_port(0x3c6, 1);
        }
        io.read_port(port, 1);
        assert_eq!(io.read_port(0x3c6, 1), 0x3f);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn high_color_uses_physical_words_and_restores_a_partly_completed_command_sequence() {
    let device = ConnectedEt4000::new(&ManualClock::default());
    let io = &device.io;
    rgb555_640(io);
    let layout = device.card.borrow().packed_layout().unwrap();
    assert_eq!(layout.format(), PixelFormat::Rgb555);
    assert_eq!(layout.size(), [640, 480]);
    assert_eq!(layout.pitch(), 1280);
    io.write_mmio(0xafffe, 2, 0x801f);
    io.write_port(0x3cd, 1, 0x11);
    io.write_mmio(0xa0000, 2, 0x7c00);
    {
        let mut card = device.card.borrow_mut();
        let mut frame = card.framebuffer();
        let pixel = frame.pixel(127, 51).unwrap();
        assert_eq!(pixel.value(), 0x801f);
        assert_eq!(frame.pixel(128, 51).unwrap().value(), 0x7c00);
        frame.write_pixel(&pixel, pixel.value() ^ pixel.color_mask());
    }
    io.write_port(0x3cd, 1, 0);
    assert_eq!(io.read_mmio(0xafffe, 2), 0xffe0);
    io.with_context(|mut context| {
        let mut card = device.card.borrow_mut();
        assert!(card.set_packed_pitch(context.reborrow(), 1288).is_some());
        assert!(card.set_display_origin(context, [1, 0]));
    });
    assert_eq!(
        device
            .card
            .borrow()
            .packed_layout()
            .unwrap()
            .display_start(),
        2
    );
    io.read_port(0x3c8, 1);
    io.read_port(0x3c6, 1);
    io.read_port(0x3c6, 1);
    let saved = Et4000State::from_bytes(&device.card.borrow().et4000_state().to_bytes()).unwrap();
    command(io, 0);
    io.with_context(|context| {
        device
            .card
            .borrow_mut()
            .restore_state(context, None, None, Some(saved));
    });
    for _ in 0..2 {
        assert_eq!(io.read_port(0x3c6, 1), 0xff);
    }
    assert_eq!(io.read_port(0x3c6, 1), 0x80);
    io.write_port(0x3c6, 1, 0xa0);
    // Mode 2 pairs consecutive bytes from ordinary byte output.
    io.write_port(0x3c4, 2, 0xbc07);
    io.read_port(0x3da, 1);
    io.write_port(0x3c0, 1, 0x36);
    io.write_port(0x3c0, 1, 0);
    io.write_port(0x3d4, 2, 0x5013);
    assert_eq!(
        device.card.borrow().packed_layout().unwrap().size(),
        [320, 480]
    );
    assert_eq!(io.read_mmio(0xafffe, 2), 0xffe0);
    io.with_context(|context| device.card.borrow_mut().prepare_legacy_mode(context));
    for _ in 0..4 {
        io.read_port(0x3c6, 1);
    }
    assert_eq!(io.read_port(0x3c6, 1), 0);
}
