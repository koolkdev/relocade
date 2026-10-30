use super::{common, pixel, read_pixels, set_cell};
use futures_util::FutureExt;
use relocade_machine::Device;
use relocade_peripherals::display::DisplayRenderer;
use relocade_vga::{
    registers::{crtc::CURSOR_START, COLOR_INPUT_STATUS_1_PORT},
    VgaDisplay,
};
use std::rc::Rc;

async fn check_display() {
    let instance = wgpu::Instance::default();
    let adapter = instance.request_adapter(&Default::default()).await.unwrap();
    let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
    let clock = Rc::new(common::ManualClock::default());
    let mut vga = common::text_vga(clock.clone());
    common::crtc(&mut vga, CURSOR_START, 0x20);
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
        "another redraw must reuse decoded pixels"
    );

    set_cell(&mut vga, 0, 0xdb, 1);
    clock.set(common::boundary(2));
    vga.read_port(COLOR_INPUT_STATUS_1_PORT, 1);
    assert_eq!(display.update(&queue).await, Some(true));
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
