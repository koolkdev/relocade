use super::*;
use relocade_vga::{PackedLayout, Scanout};

// Data-book table 3.2.4-3, mode 2Eh. No DISPI mode call or host geometry setter.
pub(super) fn indexed_640(io: &IoBus) {
    io.write_port(0x3bf, 1, 3);
    io.write_port(0x3d8, 1, 0xa0);
    io.write_port(0x3c2, 1, 0xe3);
    for value in [0x0300, 0x0101, 0x0f02, 0x0003, 0x0e04] {
        io.write_port(0x3c4, 2, value);
    }
    for value in [0x0000, 0x0001, 0x0003, 0x0004, 0x4005, 0x0506, 0xff08] {
        io.write_port(0x3ce, 2, value);
    }
    io.write_port(0x3d4, 2, 0x0011);
    for (index, value) in [
        0x5f, 0x4f, 0x50, 0x82, 0x54, 0x80, 0x0b, 0x3e, 0x00, 0x40, 0, 0, 0, 0, 0, 0, 0xea, 0x8c,
        0xdf, 0x50, 0x60, 0xe7, 0x04, 0xab, 0xff,
    ]
    .into_iter()
    .enumerate()
    {
        io.write_port(0x3d4, 2, (value << 8) | index as u32);
    }
    io.write_port(0x3d4, 2, 0x0035);
    io.read_port(0x3da, 1);
    io.write_port(0x3c0, 1, 0x30);
    io.write_port(0x3c0, 1, 1);
}

pub(super) fn layout(device: &ConnectedEt4000) -> PackedLayout {
    match *device.card.borrow().snapshot().scanout() {
        Scanout::Packed(layout) => layout,
        Scanout::Vga(_) => panic!("expected register-derived packed scanout"),
    }
}

pub(super) fn legacy_indexed_split(io: &IoBus) {
    indexed_640(io);
    io.write_port(0x3cd, 1, 0);
    io.write_port(0x3d4, 2, 0x0033);
    io.write_port(0x3d4, 2, 0x0011);
    // Mode 13h, with start counter one and a split after physical line zero.
    for (index, value) in [
        0x5f, 0x4f, 0x50, 0x82, 0x54, 0x80, 0xbf, 0x0f, 0, 1, 0, 0, 0, 1, 0, 0, 0x9c, 0x8e, 0x8f,
        0x28, 0x40, 0x96, 0xb9, 0xa3, 0,
    ]
    .into_iter()
    .enumerate()
    {
        io.write_port(0x3d4, 2, (value << 8) | index as u32);
    }
    io.read_port(0x3da, 1);
    io.write_port(0x3c0, 1, 0x30);
    io.write_port(0x3c0, 1, 0x41);
    io.write_mmio(0xa0000, 4, 0x01010101);
    io.write_mmio(0xa0004, 4, 0x02020202);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn legacy_indexed_output_retains_vga_scanline_and_split_rules() {
    let device = ConnectedEt4000::new(&ManualClock::default());
    legacy_indexed_split(&device.io);
    let mut card = device.card.borrow_mut();
    let snapshot = card.snapshot();
    assert!(snapshot.vga().is_some());
    // The split cuts a doubled row, so the VGA decoder retains all scanlines.
    assert_eq!(snapshot.geometry().size, [320, 400]);
    let framebuffer = card.framebuffer();
    assert_eq!(framebuffer.pixel(0, 0).unwrap().value(), 2);
    assert_eq!(framebuffer.pixel(0, 1).unwrap().value(), 1);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn standard_and_extended_registers_describe_the_indexed_image() {
    let clock = ManualClock::default();
    let device = ConnectedEt4000::new(&clock);
    let io = &device.io;
    indexed_640(io);
    assert_eq!(layout(&device).size(), [640, 480]);
    assert_eq!(layout(&device).pitch(), 640);
    // A row crossing a CPU bank boundary remains contiguous on the display.
    io.write_mmio(0xafffe, 2, 0x2211);
    io.write_port(0x3cd, 1, 1);
    io.write_mmio(0xa0000, 2, 0x4433);
    let mut card = device.card.borrow_mut();
    let fb = card.framebuffer();
    for (x, value) in [(254, 0x11), (255, 0x22), (256, 0x33), (257, 0x44)] {
        assert_eq!(fb.pixel(x, 102).unwrap().value(), value);
    }
    drop(card);
    // Pitch and fine pan are register inputs, independent of CPU bank pointers.
    io.write_port(0x3d4, 2, 0x6413);
    io.read_port(0x3da, 1);
    io.write_port(0x3c0, 1, 0x33);
    io.write_port(0x3c0, 1, 3);
    assert_eq!(layout(&device).pitch(), 800);
    assert_eq!(layout(&device).display_start(), 3);
    io.write_port(0x3cd, 1, 0xf7);
    assert_eq!(layout(&device).display_start(), 3);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn wrapped_viewports_and_retained_drawing_locations_address_physical_bytes() {
    let clock = ManualClock::default();
    let device = ConnectedEt4000::new(&clock);
    let io = &device.io;
    indexed_640(io);
    io.write_port(0x3cd, 1, 15);
    io.write_mmio(0xafffc, 4, 0x44332211);
    io.write_port(0x3cd, 1, 0);
    io.write_mmio(0xa0000, 1, 0x55);
    io.write_port(0x3d4, 2, 0x0333);
    io.write_port(0x3d4, 2, 0xff0c);
    io.write_port(0x3d4, 2, 0xff0d);
    let saved = {
        let mut card = device.card.borrow_mut();
        let fb = card.framebuffer();
        for (x, expected) in [0x11, 0x22, 0x33, 0x44, 0x55].into_iter().enumerate() {
            assert_eq!(fb.pixel(x as u16, 0).unwrap().value(), expected);
        }
        fb.pixel(4, 0).unwrap()
    };
    clock.advance_to(clock.at(Duration::from_millis(20)));
    io.read_port(0x3da, 1);
    assert_eq!(layout(&device).display_start(), 0xffffc);
    assert_eq!(layout(&device).pixel_address(4, 0), 0);
    io.write_port(0x3d4, 2, 0x000c);
    io.write_port(0x3d4, 2, 0x000d);
    device
        .card
        .borrow_mut()
        .framebuffer()
        .write_pixel(&saved, 0x77);
    assert_eq!(io.read_mmio(0xa0000, 1), 0x77);
}
