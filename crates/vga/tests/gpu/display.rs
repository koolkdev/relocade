use super::{common, pixel, read_pixels, set_cell};
use futures_util::FutureExt;
use relocade_frontend::display::DisplayRenderer;
use relocade_machine::Device;
use relocade_vga::{
    registers::{attribute, crtc, COLOR_INPUT_STATUS_1_PORT},
    VgaDisplay,
};
use std::rc::Rc;

async fn check_display() {
    let instance = wgpu::Instance::default();
    let adapter = instance.request_adapter(&Default::default()).await.unwrap();
    let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
    let clock = Rc::new(common::ManualClock::default());
    let mut vga = common::text_vga(clock.clone());
    common::crtc(&mut vga, crtc::CURSOR_START, 0x20);
    set_cell(&mut vga, 0, 0xdb, 4);
    let source = vga.frame_source();
    let mut display = VgaDisplay::new(&device, source.clone()).unwrap();

    let mut encoder = device.create_command_encoder(&Default::default());
    assert!(display.encode_image(&mut encoder).is_none());
    queue.submit([encoder.finish()]);
    // The host can cancel an input wait to service a redraw without losing input.
    assert!(display.update(&queue).now_or_never().is_none());
    clock.set(common::boundary(1));
    vga.read_port(COLOR_INPUT_STATUS_1_PORT, 1);
    assert_eq!(display.update(&queue).await, Some(true));
    queue.submit([]);
    assert!(
        display.current_frame().is_none(),
        "upload does not decode an image"
    );

    let mut encoder = device.create_command_encoder(&Default::default());
    let image = display.encode_image(&mut encoder).unwrap().clone();
    queue.submit([encoder.finish()]);
    let pixels = read_pixels(&device, &queue, image.texture()).await;
    assert_eq!(pixel(&pixels, 0, 0), [170, 0, 0, 255]);
    let first = source.current_capture().unwrap();
    assert_eq!(display.current_frame().unwrap().sequence, first.sequence);
    assert_eq!(display.current_frame().unwrap().time, first.time);

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
    queue.submit([]);
    for frame in 2..=8 {
        clock.set(common::boundary(frame));
        vga.read_port(COLOR_INPUT_STATUS_1_PORT, 1);
        assert_eq!(display.update(&queue).await, Some(false));
        queue.submit([]);
    }
    assert_eq!(source.current_capture().unwrap().sequence, 8);
    let mut encoder = device.create_command_encoder(&Default::default());
    assert_eq!(
        display.encode_image(&mut encoder).unwrap().texture(),
        image.texture()
    );
    queue.submit([encoder.finish()]);
    let pixels = read_pixels(&device, &queue, image.texture()).await;
    assert_eq!(
        pixel(&pixels, 0, 0),
        [0, 255, 255, 255],
        "unchanged captures and another redraw must reuse decoded pixels"
    );
    assert_eq!(display.current_frame().unwrap().sequence, first.sequence);

    set_cell(&mut vga, 0, 0xdb, 1);
    clock.set(common::boundary(9));
    vga.read_port(COLOR_INPUT_STATUS_1_PORT, 1);
    assert_eq!(display.update(&queue).await, Some(true));
    queue.submit([]);
    // An unchanged capture must not cancel decoding that is still pending.
    clock.set(common::boundary(10));
    vga.read_port(COLOR_INPUT_STATUS_1_PORT, 1);
    assert_eq!(display.update(&queue).await, Some(false));
    queue.submit([]);
    let pixels = read_pixels(&device, &queue, image.texture()).await;
    assert_eq!(
        pixel(&pixels, 0, 0),
        [0, 255, 255, 255],
        "new inputs must leave existing pixels intact"
    );
    let mut encoder = device.create_command_encoder(&Default::default());
    let latest = display.encode_image(&mut encoder).unwrap().clone();
    queue.submit([encoder.finish()]);
    let pixels = read_pixels(&device, &queue, latest.texture()).await;
    assert_eq!(pixel(&pixels, 0, 0), [0, 0, 170, 255]);
    let last = source.current_capture().unwrap();
    assert_eq!(display.current_frame().unwrap().sequence, last.sequence);
    assert_eq!(display.current_frame().unwrap().time, last.time);

    drop(vga);
    assert_eq!(display.update(&queue).await, None);
    let mut encoder = device.create_command_encoder(&Default::default());
    assert_eq!(
        display.encode_image(&mut encoder).unwrap().texture(),
        latest.texture()
    );
    queue.submit([encoder.finish()]);
    let pixels = read_pixels(&device, &queue, latest.texture()).await;
    assert_eq!(pixel(&pixels, 0, 0), [0, 0, 170, 255]);
    // The same connection must restore its closed source on a replacement GPU,
    // without another guest write or a new capture sequence.
    display.device_lost();
    assert!(display.current_frame().is_none());
    drop((image, latest));
    device.destroy();
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    let adapter = instance.request_adapter(&Default::default()).await.unwrap();
    let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
    display.attach_device(&device, &queue).unwrap();
    queue.submit([]);
    assert!(
        display.current_frame().is_none(),
        "replacement uploads without decoding"
    );
    let mut encoder = device.create_command_encoder(&Default::default());
    let restored = display.encode_image(&mut encoder).unwrap().clone();
    queue.submit([encoder.finish()]);
    let pixels = read_pixels(&device, &queue, restored.texture()).await;
    assert_eq!(pixel(&pixels, 0, 0), [0, 0, 170, 255]);
    assert_eq!(display.current_frame().unwrap().sequence, last.sequence);
    assert_eq!(display.current_frame().unwrap().time, last.time);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn display_connection_uploads_on_update_and_reuses_decoded_pixels() {
    pollster::block_on(check_display());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn display_connection_uploads_on_update_and_reuses_decoded_pixels() {
    check_display().await;
}

async fn check_display_state_changes() {
    let instance = wgpu::Instance::default();
    let adapter = instance.request_adapter(&Default::default()).await.unwrap();
    let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
    let clock = Rc::new(common::ManualClock::default());
    let mut vga = common::text_vga(clock.clone());
    common::support::registers::attribute(&mut vga, attribute::MODE_CONTROL, 0x08);
    set_cell(&mut vga, 0, 0xdb, 0x84); // Blinking red glyph.
    set_cell(&mut vga, 1, b' ', 2); // Green cursor over a blank cell.
    common::crtc(&mut vga, crtc::CURSOR_ADDRESS_LOW, 1);
    common::crtc(&mut vga, crtc::CURSOR_START, 6);
    common::crtc(&mut vga, crtc::CURSOR_END, 7);
    let mut display = VgaDisplay::new(&device, vga.frame_source()).unwrap();

    // Only the blink phase changes; no guest writes occur between captures.
    for (frame, changed, glyph, cursor) in [
        (1, true, [170, 0, 0, 255], [0, 170, 0, 255]),
        (2, false, [170, 0, 0, 255], [0, 170, 0, 255]),
        (16, true, [0, 0, 0, 255], [0, 0, 0, 255]),
        (17, false, [0, 0, 0, 255], [0, 0, 0, 255]),
        (32, true, [170, 0, 0, 255], [0, 170, 0, 255]),
    ] {
        clock.set(common::boundary(frame));
        vga.read_port(COLOR_INPUT_STATUS_1_PORT, 1);
        assert_eq!(display.update(&queue).await, Some(changed));
        let mut encoder = device.create_command_encoder(&Default::default());
        let image = display.encode_image(&mut encoder).unwrap().clone();
        queue.submit([encoder.finish()]);
        let pixels = read_pixels(&device, &queue, image.texture()).await;
        assert_eq!(pixel(&pixels, 0, 0), glyph);
        assert_eq!(pixel(&pixels, 8, 6), cursor);
    }

    // Palette and geometry changes must also redraw with unchanged VRAM.
    common::support::registers::palette(&mut vga, 4, [0, 0, 63]);
    clock.set(common::boundary(33));
    vga.read_port(COLOR_INPUT_STATUS_1_PORT, 1);
    assert_eq!(display.update(&queue).await, Some(true));
    let mut encoder = device.create_command_encoder(&Default::default());
    let image = display.encode_image(&mut encoder).unwrap().clone();
    queue.submit([encoder.finish()]);
    let pixels = read_pixels(&device, &queue, image.texture()).await;
    assert_eq!(pixel(&pixels, 0, 0), [0, 0, 255, 255]);

    let retrace_end = vga.snapshot().registers.crtc.values[crtc::VERTICAL_RETRACE_END as usize];
    common::crtc(&mut vga, crtc::VERTICAL_RETRACE_END, retrace_end & 0x7f);
    common::crtc(&mut vga, crtc::HORIZONTAL_DISPLAY_END, 39);
    common::crtc(&mut vga, crtc::VERTICAL_RETRACE_END, retrace_end);
    clock.set(common::boundary(34));
    vga.read_port(COLOR_INPUT_STATUS_1_PORT, 1);
    assert_eq!(display.update(&queue).await, Some(true));
    let mut encoder = device.create_command_encoder(&Default::default());
    let resized = display.encode_image(&mut encoder).unwrap().clone();
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
