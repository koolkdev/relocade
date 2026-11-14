use super::*;
use relocade_vga::{DisplaySnapshot, FramebufferGeometry};

fn planar_1024(io: &IoBus) {
    scanout::indexed_640(io);
    io.write_port(0x3c4, 2, 0x0604); // Four planes, without CPU chain-4.
    io.write_port(0x3ce, 2, 0x0005);
    io.write_port(0x3d4, 2, 0x0011);
    // A 1024x768 plane image needs 96 KiB per plane. Keep the split below it.
    for (index, value) in [
        0xa3, 0x7f, 0x80, 0x86, 0x83, 0x94, 0x24, 0xfd, 0, 0x60, 0, 0, 0, 0, 0, 0, 3, 9, 0xff,
        0x40, 0, 0, 0x25, 0xe3, 0xff,
    ]
    .into_iter()
    .enumerate()
    {
        io.write_port(0x3d4, 2, (value << 8) | index as u32);
    }
    io.write_port(0x3d4, 2, 0x1035);
    for color in 0..16 {
        support::registers::attribute(io, color, color);
    }
    support::registers::attribute(io, 0x10, 1);
    support::registers::attribute(io, 0x12, 15);
    support::registers::attribute(io, 0x13, 0);
    io.write_port(0x3c8, 1, 0);
    for component in [0, 0, 0, 63, 0, 0, 0, 63, 0] {
        io.write_port(0x3c9, 1, component);
    }
}

fn write_plane(io: &IoBus, bank: u8, plane: u8, offset: u16, value: u8) {
    io.write_port(0x3cd, 1, u32::from(bank));
    io.write_port(0x3c4, 2, (1 << (8 + plane)) | 2);
    io.write_mmio(0xa0000 + u32::from(offset), 1, u64::from(value));
}

fn plane_rows_display() -> DisplaySnapshot {
    let device = ConnectedEt4000::new(&ManualClock::default());
    planar_1024(&device.io);
    write_plane(&device.io, 0, 0, 0, 0x80);
    write_plane(&device.io, 1, 1, 0, 0x80);
    device.io.write_port(0x3cd, 1, 0xf7);
    let mut card = device.card.borrow_mut();
    let framebuffer = card.framebuffer();
    assert_eq!(
        framebuffer.geometry(),
        Some(FramebufferGeometry::Graphics {
            width: 1024,
            height: 768
        })
    );
    assert_eq!(framebuffer.pixel(0, 0).unwrap().value(), 1);
    assert_eq!(framebuffer.pixel(0, 512).unwrap().value(), 2);
    let pixel = framebuffer.pixel(1, 512).unwrap();
    card.framebuffer().write_pixel(&pixel, 1);
    let snapshot = card.snapshot();
    assert_eq!(snapshot.geometry().size, [1024, 768]);
    assert_eq!(snapshot.vram_byte(0x40000), Some(0x40));
    assert_eq!(snapshot.vram_byte(0x40001), Some(0x80));
    assert_eq!(snapshot.vram_byte(0xfffff), Some(0));
    assert_eq!(snapshot.vram_byte(0x100000), None);
    snapshot
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn planar_rows_cross_the_old_plane_boundary_independently_of_cpu_banks() {
    plane_rows_display();
}

fn wrapped_display(clock: &ManualClock) -> DisplaySnapshot {
    let device = ConnectedEt4000::new(clock);
    planar_1024(&device.io);
    write_plane(&device.io, 3, 0, 0xffff, 0x80);
    write_plane(&device.io, 0, 1, 0, 0x80);
    for value in [0x0333, 0xff0c, 0xff0d] {
        device.io.write_port(0x3d4, 2, value);
    }
    {
        let mut card = device.card.borrow_mut();
        let framebuffer = card.framebuffer();
        assert_eq!(framebuffer.pixel(0, 0).unwrap().value(), 1);
        assert_eq!(framebuffer.pixel(8, 0).unwrap().value(), 2);
        assert_eq!(card.snapshot().vga().unwrap().display_start, 0);
    }
    clock.advance_to(clock.at(Duration::from_millis(100)));
    device.io.read_port(0x3da, 1);
    let captured = device.card.borrow().snapshot();
    assert_eq!(captured.vga().unwrap().display_start, 0x3ffff);
    write_plane(&device.io, 3, 0, 0xffff, 0);
    assert_eq!(captured.vram_byte(0xffffc), Some(0x80));
    captured
}

fn split_display(clock: &ManualClock) -> DisplaySnapshot {
    let device = ConnectedEt4000::new(clock);
    planar_1024(&device.io);
    // An 8x1026 image, split after count 1024, with byte and pixel panning.
    for (index, value) in [
        (1, 0),
        (6, 0x20),
        (7, 0),
        (8, 0x20),
        (9, 0),
        (0x10, 0x18),
        (0x12, 1),
        (0x13, 1),
        (0x15, 0x10),
        (0x16, 0x21),
        (0x18, 0),
        (0x33, 1),
        (0x35, 0x1f),
    ] {
        support::registers::indexed(&device.io, 0x3d4, index, value);
    }
    support::registers::attribute(&device.io, 0x10, 0x21); // Reset both pans after the split.
    support::registers::attribute(&device.io, 0x13, 1);
    write_plane(&device.io, 1, 0, 0x801, 0x40);
    write_plane(&device.io, 0, 1, 0, 0x80);
    {
        let mut card = device.card.borrow_mut();
        let framebuffer = card.framebuffer();
        assert_eq!(framebuffer.pixel(0, 1024).unwrap().value(), 1);
        assert_eq!(framebuffer.pixel(0, 1025).unwrap().value(), 2);
    }
    clock.advance_to(clock.at(Duration::from_millis(100)));
    device.io.read_port(0x3da, 1);
    let captured = device.card.borrow().snapshot();
    assert_eq!(captured.geometry().size, [8, 1026]);
    captured
}

fn word_addressing_display(clock: &ManualClock, contiguous: bool) -> DisplaySnapshot {
    let device = ConnectedEt4000::new(clock);
    planar_1024(&device.io);
    write_plane(&device.io, 2, 0, 2, 0x80);
    write_plane(&device.io, 1, 1, 1, 0x80);
    for value in [
        0xa317,
        0x0133,
        0x010d,
        if contiguous { 0x6036 } else { 0x4036 },
    ] {
        device.io.write_port(0x3d4, 2, value);
    }
    assert_eq!(
        device
            .card
            .borrow_mut()
            .framebuffer()
            .pixel(0, 0)
            .unwrap()
            .value(),
        if contiguous { 2 } else { 1 },
    );
    clock.advance_to(clock.at(Duration::from_millis(100)));
    device.io.read_port(0x3da, 1);
    let captured = device.card.borrow().snapshot();
    captured
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn full_start_wraps_at_physical_memory_and_extended_split_resets_panning() {
    wrapped_display(&ManualClock::default());
    split_display(&ManualClock::default());
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn ibm_and_tseng_mapping_interpret_the_same_programmed_counter() {
    word_addressing_display(&ManualClock::default(), false);
    word_addressing_display(&ManualClock::default(), true);
}

#[cfg(feature = "renderer")]
mod gpu {
    use super::*;
    use relocade_vga::VgaRenderer;

    async fn check() {
        let rows = plane_rows_display();
        let wrapped = wrapped_display(&ManualClock::default());
        let split = split_display(&ManualClock::default());
        let ibm = word_addressing_display(&ManualClock::default(), false);
        let tseng = word_addressing_display(&ManualClock::default(), true);
        let adapter = wgpu::Instance::default()
            .request_adapter(&Default::default())
            .await
            .unwrap();
        let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
        let mut renderer = VgaRenderer::new(&device).unwrap();
        for (snapshot, samples) in [
            (
                rows,
                [(0, 512, [0, 255, 0, 255]), (1, 512, [255, 0, 0, 255])],
            ),
            (
                wrapped.clone(),
                [(0, 0, [255, 0, 0, 255]), (8, 0, [0, 255, 0, 255])],
            ),
            (
                split,
                [(0, 1024, [255, 0, 0, 255]), (0, 1025, [0, 255, 0, 255])],
            ),
            (ibm, [(0, 0, [255, 0, 0, 255]), (1, 0, [0, 0, 0, 255])]),
            (tseng, [(0, 0, [0, 255, 0, 255]), (1, 0, [0, 0, 0, 255])]),
            (
                wrapped,
                [(0, 0, [255, 0, 0, 255]), (8, 0, [0, 255, 0, 255])],
            ),
        ] {
            let mut encoder = device.create_command_encoder(&Default::default());
            renderer.upload(&queue, &snapshot);
            let frame = renderer.encode(&mut encoder).unwrap();
            queue.submit([encoder.finish()]);
            let pixels = readback::read_pixels(&device, &queue, frame.texture()).await;
            for (x, y, expected) in samples {
                let offset = ((y * frame.size()[0] + x) * 4) as usize;
                assert_eq!(&pixels[offset..offset + 4], &expected, "({x}, {y})");
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn extended_planar_pixels_match_native_addressing() {
        pollster::block_on(check());
    }

    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test::wasm_bindgen_test]
    async fn extended_planar_pixels_match_native_addressing() {
        check().await;
    }
}
