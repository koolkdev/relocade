use super::{common, pixel, read_pixels, set_cell, text_device};
use relocade_renderer::{Image, Rect, Renderer};
use relocade_vga::VgaRenderer;
use std::time::Duration;

async fn check_recording() {
    let instance = wgpu::Instance::default();
    let adapter = instance.request_adapter(&Default::default()).await.unwrap();
    let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
    let mut vga = VgaRenderer::new(&device).unwrap();
    let mut renderer = Renderer::new(device.clone(), Image::FORMAT).unwrap();
    let output = Image::new(&device, [640, 200], (5, 12)).unwrap();
    let mut device_state = text_device();
    let mut previous_image: Option<Image> = None;
    let mut empty = device.create_command_encoder(&Default::default());
    assert!(vga.encode(&mut empty).is_none());

    for (attribute, expected, discard_first) in
        [(4, [170, 0, 0, 255], false), (1, [0, 0, 170, 255], true)]
    {
        set_cell(&mut device_state, 0, 0xdb, attribute);
        let mut input = common::fixture::capture(&device_state);
        input.sequence += u64::from(attribute);
        input.time += Duration::from_millis(1);
        vga.upload(&queue, &input);
        queue.submit([]); // Keep GPU input current without decoding or presenting.
        if let Some(image) = &previous_image {
            let pixels = read_pixels(&device, &queue, image.texture()).await;
            assert_eq!(pixel(&pixels, 0, 0), [170, 0, 0, 255]);
        }
        if discard_first {
            let mut encoder = device.create_command_encoder(&Default::default());
            let recorded = vga.encode(&mut encoder).unwrap();
            drop(encoder);
            assert_eq!(recorded.sequence, input.sequence);
            assert_eq!(recorded.time, input.time);
            // The input upload was submitted, but the discarded decode must not
            // have changed the image. Metadata is not completion.
            let pixels = read_pixels(&device, &queue, recorded.image.texture()).await;
            assert_eq!(pixel(&pixels, 0, 0), [170, 0, 0, 255]);
        }

        // The retry uses exactly the discarded capture. Recording must not cache
        // it as completed. Decode and draw then share one caller-owned submission.
        let mut encoder = device.create_command_encoder(&Default::default());
        let frame = vga.encode(&mut encoder).unwrap();
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: output.view(),
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            renderer.draw_image(&mut pass, &frame.image, Rect::from_size(output.size()));
        }
        queue.submit([encoder.finish()]);
        previous_image = Some(frame.image.clone());
        assert_eq!(frame.sequence, input.sequence);
        assert_eq!(frame.time, input.time);
        let pixels = read_pixels(&device, &queue, output.texture()).await;
        assert_eq!(pixel(&pixels, 0, 0), expected);
        assert_eq!(pixel(&pixels, 8, 0), [0, 0, 0, 255]);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn records_decode_and_draw_in_one_batch_and_retries_discarded_updates() {
    pollster::block_on(check_recording());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn records_decode_and_draw_in_one_batch_and_retries_discarded_updates() {
    check_recording().await;
}
