use super::*;
use relocade_vga::VgaRenderer;

async fn check_indexed_scanout() {
    let clock = ManualClock::default();
    let card = ConnectedEt4000::new(&clock);
    scanout::indexed_640(&card.io);
    card.io.write_port(0x3c8, 1, 1);
    for value in [63, 0, 0, 0, 63, 0] {
        card.io.write_port(0x3c9, 1, value);
    }
    card.io.write_port(0x3cd, 1, 15);
    card.io.write_mmio(0xafffc, 4, 0x01010101);
    card.io.write_port(0x3cd, 1, 0);
    card.io.write_mmio(0xa0000, 1, 2);
    for value in [0x0333, 0xff0c, 0xff0d] {
        card.io.write_port(0x3d4, 2, value);
    }
    clock.advance_to(clock.at(Duration::from_millis(20)));
    card.io.read_port(0x3da, 1);
    let snapshot = card.card.borrow().snapshot();
    let instance = wgpu::Instance::default();
    let adapter = instance.request_adapter(&Default::default()).await.unwrap();
    let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
    let mut renderer = VgaRenderer::new(&device).unwrap();
    for blanked in [false, true] {
        let mut input = snapshot.clone();
        input.blanked = blanked;
        let mut encoder = device.create_command_encoder(&Default::default());
        renderer.upload(&queue, &input);
        let frame = renderer.encode(&mut encoder).unwrap();
        assert_eq!(frame.size(), [640, 480]);
        queue.submit([encoder.finish()]);
        let pixels = readback::read_pixels(&device, &queue, frame.texture()).await;
        assert_eq!(
            &pixels[..4],
            if blanked {
                &[0, 0, 0, 255]
            } else {
                &[255, 0, 0, 255]
            }
        );
        assert_eq!(
            &pixels[16..20],
            if blanked {
                &[0, 0, 0, 255]
            } else {
                &[0, 255, 0, 255]
            }
        );
    }

    scanout::legacy_indexed_split(&card.io);
    clock.advance_to(clock.at(Duration::from_millis(40)));
    card.io.read_port(0x3da, 1);
    let mut encoder = device.create_command_encoder(&Default::default());
    renderer.upload(&queue, &card.card.borrow().snapshot());
    let frame = renderer.encode(&mut encoder).unwrap();
    assert_eq!(frame.size(), [320, 400]);
    queue.submit([encoder.finish()]);
    let pixels = readback::read_pixels(&device, &queue, frame.texture()).await;
    assert_eq!(&pixels[..4], &[0, 255, 0, 255]);
    assert_eq!(&pixels[320 * 4..320 * 4 + 4], &[255, 0, 0, 255]);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn renders_banked_bytes_and_wraps_the_physical_display_address() {
    pollster::block_on(check_indexed_scanout());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn renders_banked_bytes_and_wraps_the_physical_display_address() {
    check_indexed_scanout().await;
}

async fn check_full_pixel_blanking() {
    let clock = ManualClock::default();
    let card = ConnectedEt4000::new(&clock);
    scanout::indexed_640(&card.io);
    card.io.write_port(0x3d4, 2, 0x0c11); // Unlock CRTC timing registers.
                                          // Zero-filled VRAM is bright white, so blanking cannot pass as color zero.
    card.io.write_port(0x3c8, 1, 0);
    for value in [63; 3] {
        card.io.write_port(0x3c9, 1, value);
    }
    let frames = card.card.borrow().frames();
    let mut task = pin!(Et4000Ax::run(card.card.clone()));
    let mut cx = Context::from_waker(Waker::noop());
    assert!(task.as_mut().poll(&mut cx).is_pending());
    let mut captures = Vec::new();
    for (step, (registers, horizontal, vertical)) in [
        (&[][..], 0..0, 0..0),
        // Hblank counts 1..35, Vblank counts 3..5, within the displayed area.
        (&[(2, 0), (0x15, 2), (0x16, 4), (7, 0x36)][..], 8..280, 3..5),
        // Hblank crosses total reset; ET4000 compares vertical end bit seven.
        (
            &[(2, 97), (3, 0x81), (5, 0), (0x15, 0), (0x16, 0x82)][..],
            0..16,
            1..131,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        for &(index, value) in registers {
            support::registers::indexed(&card.io, 0x3d4, index, value);
        }
        clock.advance_to(clock.at(Duration::from_millis((step as u64 + 1) * 20)));
        assert!(task.as_mut().poll(&mut cx).is_pending());
        // Register-only blanking changes must publish a new frame as well.
        captures.push((frames.try_recv().unwrap(), horizontal, vertical));
    }
    captures.push(captures[1].clone()); // Re-upload a retained earlier capture.
    let adapter = wgpu::Instance::default()
        .request_adapter(&Default::default())
        .await
        .unwrap();
    let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
    let mut renderer = VgaRenderer::new(&device).unwrap();
    for (snapshot, horizontal, vertical) in captures {
        let mut encoder = device.create_command_encoder(&Default::default());
        renderer.upload(&queue, &snapshot);
        let frame = renderer.encode(&mut encoder).unwrap();
        assert_eq!(frame.size(), [640, 480]);
        queue.submit([encoder.finish()]);
        let pixels = readback::read_pixels(&device, &queue, frame.texture()).await;
        for (index, actual) in pixels.as_chunks::<4>().0.iter().enumerate() {
            let x = index as u32 % 640;
            let y = index as u32 / 640;
            let expected = if horizontal.contains(&x) || vertical.contains(&y) {
                [0, 0, 0, 255]
            } else {
                [255; 4]
            };
            assert_eq!(*actual, expected, "pixel ({x}, {y})");
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn packed_scanout_preserves_crtc_blanking_and_retained_captures() {
    pollster::block_on(check_full_pixel_blanking());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn packed_scanout_preserves_crtc_blanking_and_retained_captures() {
    check_full_pixel_blanking().await;
}

async fn check_high_color_scanout() {
    let clock = ManualClock::default();
    let card = ConnectedEt4000::new(&clock);
    ramdac::rgb555_640(&card.io);
    // The palette and mask are bypassed, and the top bit is not a color bit.
    card.io.write_port(0x3c6, 1, 0);
    card.io.write_port(0x3c8, 1, 0);
    for _ in 0..3 {
        card.io.write_port(0x3c9, 1, 0);
    }
    card.io.write_mmio(0xa04fe, 2, 0x001f); // Last pixel of row zero.
    card.io.write_mmio(0xa0500, 2, 0x7fff); // First pixel of row one.
    card.io.write_mmio(0xafffe, 2, 0xfc00);
    card.io.write_port(0x3cd, 1, 1);
    card.io.write_mmio(0xa0000, 2, 0x03e0); // Next pixel, across the CPU bank.
    clock.advance_to(clock.at(Duration::from_millis(40)));
    card.io.read_port(0x3da, 1);
    let high_color = card.card.borrow().snapshot();
    ramdac::command(&card.io, 0);
    let indexed = card.card.borrow().snapshot();

    let adapter = wgpu::Instance::default()
        .request_adapter(&Default::default())
        .await
        .unwrap();
    let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
    let mut renderer = VgaRenderer::new(&device).unwrap();
    for (snapshot, high_color) in [
        (high_color.clone(), true),
        (indexed, false),
        (high_color, true),
    ] {
        let mut encoder = device.create_command_encoder(&Default::default());
        renderer.upload(&queue, &snapshot);
        let frame = renderer.encode(&mut encoder).unwrap();
        assert_eq!(frame.size(), [640, 480]);
        queue.submit([encoder.finish()]);
        let pixels = readback::read_pixels(&device, &queue, frame.texture()).await;
        for (x, y, color) in [
            (639, 0, [0, 0, 255, 255]),
            (0, 1, [255; 4]),
            (127, 51, [255, 0, 0, 255]),
            (128, 51, [0, 255, 0, 255]),
        ] {
            let offset = (y * 640 + x) * 4;
            assert_eq!(
                &pixels[offset..offset + 4],
                if high_color { &color } else { &[0, 0, 0, 255] },
                "pixel ({x}, {y}), high color: {high_color}"
            );
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn rgb555_bypasses_the_palette_and_survives_mode_changes_in_retained_captures() {
    pollster::block_on(check_high_color_scanout());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn rgb555_bypasses_the_palette_and_survives_mode_changes_in_retained_captures() {
    check_high_color_scanout().await;
}
