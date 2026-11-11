use super::{common, pixel, read_pixels, set_cell};
use futures_util::FutureExt;
use relocade_clock_test_support::ManualClock;
use relocade_frontend::display::DisplayRenderer;
use relocade_machine::Device;
use relocade_vga::{
    registers::{attribute, crtc, COLOR_INPUT_STATUS_1_PORT},
    VgaDisplay,
};

async fn check_display() {
    let device_io = relocade_machine::IoBus::default();

    let instance = wgpu::Instance::default();
    let adapter = instance.request_adapter(&Default::default()).await.unwrap();
    let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
    let clock = ManualClock::default();
    let (vga, mut video) = common::start_video(common::text_vga(clock.clock()));
    common::crtc(&mut *vga.borrow_mut(), crtc::CURSOR_START, 0x20);
    set_cell(&mut *vga.borrow_mut(), 0, 0xdb, 4);
    let source = vga.borrow().frames();
    let mut display = VgaDisplay::new(&device, source.clone()).unwrap();

    let mut encoder = device.create_command_encoder(&Default::default());
    assert!(display.encode_image(&queue, &mut encoder).is_none());
    queue.submit([encoder.finish()]);
    // The host can cancel an input wait to service a redraw without losing input.
    assert!(display.service(&queue).now_or_never().is_none());
    clock.advance_to(clock.at(common::boundary(1)));
    device_io.with_context(|context| {
        vga.borrow_mut()
            .read_port(context, COLOR_INPUT_STATUS_1_PORT, 1)
    });
    common::service_video(video.as_mut());
    assert_eq!(display.service(&queue).await, Some(true));
    assert!(
        display.current_image().is_none(),
        "uploading a capture does not decode an image"
    );

    let mut encoder = device.create_command_encoder(&Default::default());
    let image = display.encode_image(&queue, &mut encoder).unwrap().clone();
    queue.submit([encoder.finish()]);
    let pixels = read_pixels(&device, &queue, image.texture()).await;
    assert_eq!(pixel(&pixels, 0, 0), [170, 0, 0, 255]);

    // Mark one output pixel to distinguish reusing the image from decoding again.
    queue.write_texture(
        image.texture().as_image_copy(),
        &[0, 255, 255, 255],
        Default::default(),
        wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
    for frame in 2..=7 {
        clock.advance_to(clock.at(common::boundary(frame)));
        device_io.with_context(|context| {
            vga.borrow_mut()
                .read_port(context, COLOR_INPUT_STATUS_1_PORT, 1)
        });
        common::service_video(video.as_mut());
        assert!(display.service(&queue).now_or_never().is_none());
    }
    assert!(source.is_empty());
    let mut encoder = device.create_command_encoder(&Default::default());
    assert_eq!(
        display
            .encode_image(&queue, &mut encoder)
            .unwrap()
            .texture(),
        image.texture()
    );
    queue.submit([encoder.finish()]);
    let pixels = read_pixels(&device, &queue, image.texture()).await;
    assert_eq!(
        pixel(&pixels, 0, 0),
        [0, 255, 255, 255],
        "unchanged captures and another redraw must reuse decoded pixels"
    );

    set_cell(&mut *vga.borrow_mut(), 0, 0xdb, 1);
    clock.advance_to(clock.at(common::boundary(9)));
    device_io.with_context(|context| {
        vga.borrow_mut()
            .read_port(context, COLOR_INPUT_STATUS_1_PORT, 1)
    });
    common::service_video(video.as_mut());
    assert_eq!(display.service(&queue).await, Some(true));
    // Publication alone cannot change the state used by independent redraws.
    // The later capture becomes visible only after it has been serviced.
    set_cell(&mut *vga.borrow_mut(), 0, 0xdb, 2);
    clock.advance_to(clock.at(common::boundary(10)));
    device_io.with_context(|context| {
        vga.borrow_mut()
            .read_port(context, COLOR_INPUT_STATUS_1_PORT, 1)
    });
    common::service_video(video.as_mut());
    clock.advance_to(clock.at(common::boundary(11)));
    device_io.with_context(|context| {
        vga.borrow_mut()
            .read_port(context, COLOR_INPUT_STATUS_1_PORT, 1)
    });
    common::service_video(video.as_mut());
    let pixels = read_pixels(&device, &queue, image.texture()).await;
    assert_eq!(
        pixel(&pixels, 0, 0),
        [0, 255, 255, 255],
        "new inputs must leave existing pixels intact"
    );
    let mut encoder = device.create_command_encoder(&Default::default());
    let uploaded = display.encode_image(&queue, &mut encoder).unwrap().clone();
    queue.submit([encoder.finish()]);
    let pixels = read_pixels(&device, &queue, uploaded.texture()).await;
    assert_eq!(pixel(&pixels, 0, 0), [0, 0, 170, 255]);

    assert_eq!(display.service(&queue).await, Some(true));
    let mut encoder = device.create_command_encoder(&Default::default());
    let latest = display.encode_image(&queue, &mut encoder).unwrap().clone();
    queue.submit([encoder.finish()]);
    let pixels = read_pixels(&device, &queue, latest.texture()).await;
    assert_eq!(pixel(&pixels, 0, 0), [0, 170, 0, 255]);

    assert!(display.service(&queue).now_or_never().is_none());

    drop(video);
    drop(vga);
    assert_eq!(display.service(&queue).await, None);
    let mut encoder = device.create_command_encoder(&Default::default());
    assert_eq!(
        display
            .encode_image(&queue, &mut encoder)
            .unwrap()
            .texture(),
        latest.texture()
    );
    queue.submit([encoder.finish()]);
    let pixels = read_pixels(&device, &queue, latest.texture()).await;
    assert_eq!(pixel(&pixels, 0, 0), [0, 170, 0, 255]);
    // The same connection must restore its closed source on a replacement GPU,
    // without another guest write or snapshot.
    display.device_lost();
    assert!(display.current_image().is_none());
    drop((image, uploaded, latest));
    device.destroy();
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    let adapter = instance.request_adapter(&Default::default()).await.unwrap();
    let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
    display.attach_device(&device, &queue).unwrap();
    assert!(
        display.current_image().is_none(),
        "replacement waits for redraw before restoring pixels"
    );
    let mut encoder = device.create_command_encoder(&Default::default());
    let restored = display.encode_image(&queue, &mut encoder).unwrap().clone();
    queue.submit([encoder.finish()]);
    let pixels = read_pixels(&device, &queue, restored.texture()).await;
    assert_eq!(pixel(&pixels, 0, 0), [0, 170, 0, 255]);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn display_connection_services_updates_and_reuses_decoded_pixels() {
    pollster::block_on(check_display());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn display_connection_services_updates_and_reuses_decoded_pixels() {
    check_display().await;
}

async fn check_display_state_changes() {
    let device_io = relocade_machine::IoBus::default();

    let instance = wgpu::Instance::default();
    let adapter = instance.request_adapter(&Default::default()).await.unwrap();
    let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
    let clock = ManualClock::default();
    let (vga, mut video) = common::start_video(common::text_vga(clock.clock()));
    common::support::registers::attribute(&mut *vga.borrow_mut(), attribute::MODE_CONTROL, 0x08);
    set_cell(&mut *vga.borrow_mut(), 0, 0xdb, 0x84); // Blinking red glyph.
    set_cell(&mut *vga.borrow_mut(), 1, b' ', 0x82); // Green cursor over a blinking blank cell.
    common::crtc(&mut *vga.borrow_mut(), crtc::CURSOR_ADDRESS_LOW, 1);
    common::crtc(&mut *vga.borrow_mut(), crtc::CURSOR_START, 6);
    common::crtc(&mut *vga.borrow_mut(), crtc::CURSOR_END, 7);
    let mut display = VgaDisplay::new(&device, vga.borrow().frames()).unwrap();

    // Only blink phases change; no guest writes occur between captures.
    for (frame, changed, glyph, cursor) in [
        (1, true, [170, 0, 0, 255], [0, 170, 0, 255]),
        (7, false, [170, 0, 0, 255], [0, 170, 0, 255]),
        (8, true, [170, 0, 0, 255], [0, 0, 0, 255]),
        (15, false, [170, 0, 0, 255], [0, 0, 0, 255]),
        (16, true, [0, 0, 0, 255], [0, 170, 0, 255]),
        (23, false, [0, 0, 0, 255], [0, 170, 0, 255]),
        (24, true, [0, 0, 0, 255], [0, 0, 0, 255]),
        (31, false, [0, 0, 0, 255], [0, 0, 0, 255]),
        (32, true, [170, 0, 0, 255], [0, 170, 0, 255]),
    ] {
        clock.advance_to(clock.at(common::boundary(frame)));
        device_io.with_context(|context| {
            vga.borrow_mut()
                .read_port(context, COLOR_INPUT_STATUS_1_PORT, 1)
        });
        common::service_video(video.as_mut());
        if changed {
            assert_eq!(display.service(&queue).await, Some(true));
        } else {
            assert!(display.service(&queue).now_or_never().is_none());
        }
        let mut encoder = device.create_command_encoder(&Default::default());
        let image = display.encode_image(&queue, &mut encoder).unwrap().clone();
        queue.submit([encoder.finish()]);
        let pixels = read_pixels(&device, &queue, image.texture()).await;
        assert_eq!(pixel(&pixels, 0, 0), glyph);
        assert_eq!(pixel(&pixels, 8, 6), cursor);
    }

    // Palette and geometry changes must also redraw with unchanged VRAM.
    common::support::registers::palette(&mut *vga.borrow_mut(), 4, [0, 0, 63]);
    clock.advance_to(clock.at(common::boundary(33)));
    device_io.with_context(|context| {
        vga.borrow_mut()
            .read_port(context, COLOR_INPUT_STATUS_1_PORT, 1)
    });
    common::service_video(video.as_mut());
    assert_eq!(display.service(&queue).await, Some(true));
    let mut encoder = device.create_command_encoder(&Default::default());
    let image = display.encode_image(&queue, &mut encoder).unwrap().clone();
    queue.submit([encoder.finish()]);
    let pixels = read_pixels(&device, &queue, image.texture()).await;
    assert_eq!(pixel(&pixels, 0, 0), [0, 0, 255, 255]);

    let retrace_end =
        vga.borrow().snapshot().vga().unwrap().crtc.values[crtc::VERTICAL_RETRACE_END as usize];
    common::crtc(
        &mut *vga.borrow_mut(),
        crtc::VERTICAL_RETRACE_END,
        retrace_end & 0x7f,
    );
    common::crtc(&mut *vga.borrow_mut(), crtc::HORIZONTAL_DISPLAY_END, 39);
    common::crtc(
        &mut *vga.borrow_mut(),
        crtc::VERTICAL_RETRACE_END,
        retrace_end,
    );
    clock.advance_to(clock.at(common::boundary(34)));
    device_io.with_context(|context| {
        vga.borrow_mut()
            .read_port(context, COLOR_INPUT_STATUS_1_PORT, 1)
    });
    common::service_video(video.as_mut());
    assert_eq!(display.service(&queue).await, Some(true));
    let mut encoder = device.create_command_encoder(&Default::default());
    let resized = display.encode_image(&queue, &mut encoder).unwrap().clone();
    queue.submit([encoder.finish()]);
    assert_eq!(resized.size(), [320, 200]);
    assert_ne!(resized.texture(), image.texture());
    let pixels = read_pixels(&device, &queue, resized.texture()).await;
    assert_eq!(&pixels[..4], [0, 0, 255, 255]);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn display_connection_redraws_for_blink_palette_and_geometry_changes() {
    pollster::block_on(check_display_state_changes());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn display_connection_redraws_for_blink_palette_and_geometry_changes() {
    check_display_state_changes().await;
}
