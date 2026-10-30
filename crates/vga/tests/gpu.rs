#![cfg(feature = "renderer")]

#[path = "gpu/blanking.rs"]
mod blanking;
mod common;
#[path = "gpu/graphics.rs"]
mod graphics;
#[path = "gpu/readback.rs"]
mod readback;
use readback::read_pixels;
#[path = "gpu/indexed256.rs"]
mod indexed256;
#[path = "gpu/parity.rs"]
mod parity;
#[path = "gpu/planar16.rs"]
mod planar16;
#[path = "gpu/recording.rs"]
mod recording;
#[path = "gpu/unchained256.rs"]
mod unchained256;
use common::support::software;

use relocade_vga::{CapturedFrame, VgaRenderer};
use std::{sync::Arc, time::Duration};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

use common::fixture::{set_cell, text_device};
fn capture() -> CapturedFrame {
    common::fixture::capture(&text_device())
}

fn pixel(pixels: &[u8], x: usize, y: usize) -> &[u8] {
    let offset = (y * 640 + x) * 4;
    &pixels[offset..offset + 4]
}

async fn check_image() {
    let instance = wgpu::Instance::default();
    let adapter = instance.request_adapter(&Default::default()).await.unwrap();
    let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
    let mut renderer = VgaRenderer::new(&device).unwrap();
    let mut vga = text_device();
    set_cell(&mut vga, 0, b'F', 0x6f);
    set_cell(&mut vga, 1999, 0xdb, 0x09);
    let input = common::fixture::capture(&vga);
    let input = Arc::new(input);
    let mut encoder = device.create_command_encoder(&Default::default());
    renderer.upload(&queue, &input);
    let frame = renderer.encode(&mut encoder).unwrap();
    queue.submit([encoder.finish()]);
    assert_eq!(frame.image.size(), [640, 200]);
    assert_eq!(frame.image.pixel_aspect_ratio(), (5, 12));
    assert_eq!(frame.time, input.time);
    assert_eq!(frame.sequence, input.sequence);
    let pixels = read_pixels(&device, &queue, frame.image.texture()).await;
    // Literal font rows and palette values check packing, glyph direction and
    // colors independently of the software conversion implementation.
    for (y, bits) in [0xfe, 0x62, 0x68, 0x78, 0x68, 0x60, 0xf0, 0]
        .into_iter()
        .enumerate()
    {
        for x in 0..8 {
            let expected = if bits & (0x80 >> x) != 0 {
                [255, 255, 255, 255]
            } else {
                [170, 85, 0, 255]
            };
            assert_eq!(pixel(&pixels, x, y), expected);
        }
    }
    assert_eq!(pixel(&pixels, 8, 0), [0, 0, 0, 255]);
    assert_eq!(pixel(&pixels, 639, 199), [85, 85, 255, 255]);
    let image = frame.image.clone();

    // Another capture updates reusable storage, including retained Image handles,
    // even when the sequence is unchanged.
    let mut vga = text_device();
    set_cell(&mut vga, 0, 0xdb, 4);
    let mut other = common::fixture::capture(&vga);
    other.time += Duration::from_secs(1);
    let other = Arc::new(other);
    let mut encoder = device.create_command_encoder(&Default::default());
    renderer.upload(&queue, &other);
    let frame = renderer.encode(&mut encoder).unwrap();
    queue.submit([encoder.finish()]);
    assert_eq!(frame.sequence, input.sequence);
    assert_eq!(frame.time, other.time);
    assert_eq!(frame.image.texture(), image.texture());
    let pixels = read_pixels(&device, &queue, image.texture()).await;
    assert_eq!(pixel(&pixels, 0, 0), [170, 0, 0, 255]);
    assert_eq!(pixel(&pixels, 639, 199), [0, 0, 0, 255]);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn renders_captures_into_reusable_gpu_storage() {
    pollster::block_on(check_image());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn renders_captures_into_reusable_gpu_storage() {
    check_image().await;
}
