#![cfg(feature = "renderer")]

mod common;
#[path = "gpu/parity.rs"]
mod parity;
#[path = "gpu/recording.rs"]
mod recording;
#[path = "support/software.rs"]
mod software;

use relocade_vga::{CapturedFrame, TextCell, TextCursor, TextScreen, VgaRenderer};
use std::{sync::Arc, time::Duration};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

fn capture() -> CapturedFrame {
    CapturedFrame {
        screen: TextScreen {
            cells: [TextCell {
                character: b' ',
                attribute: 7,
            }; TextScreen::COLUMNS * TextScreen::ROWS],
            cursor: TextCursor {
                address: 0,
                start_scan_line: 0x20,
                end_scan_line: 7,
            },
            start_address: 0,
        },
        blink_on: true,
        blanked: false,
        time: Duration::from_millis(123),
        sequence: 42,
    }
}

async fn read_pixels(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
) -> Vec<u8> {
    let row_bytes = texture.width() * 4;
    let stride =
        row_bytes.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("VGA test readback"),
        size: u64::from(stride) * u64::from(texture.height()),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(stride),
                rows_per_image: Some(texture.height()),
            },
        },
        texture.size(),
    );
    queue.submit([encoder.finish()]);
    let (send, receive) = futures_channel::oneshot::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            send.send(result).unwrap();
        });
    #[cfg(not(target_arch = "wasm32"))]
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    receive.await.unwrap().unwrap();
    let mapped = buffer.slice(..).get_mapped_range().unwrap();
    let pixels = mapped
        .chunks_exact(stride as usize)
        .flat_map(|row| row[..row_bytes as usize].iter().copied())
        .collect();
    drop(mapped);
    buffer.unmap();
    pixels
}

fn pixel(pixels: &[u8], x: usize, y: usize) -> &[u8] {
    let offset = (y * 640 + x) * 4;
    &pixels[offset..offset + 4]
}

async fn check_image() {
    let instance = wgpu::Instance::default();
    let adapter = instance.request_adapter(&Default::default()).await.unwrap();
    let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
    let mut renderer = VgaRenderer::new(&device, &queue).unwrap();
    let mut input = capture();
    input.screen.cells[0] = TextCell {
        character: b'F',
        attribute: 0x6f,
    };
    input.screen.cells[1999] = TextCell {
        character: 0xdb,
        attribute: 0x09,
    };
    let input = Arc::new(input);
    let mut encoder = device.create_command_encoder(&Default::default());
    let frame = renderer.encode(&queue, &mut encoder, &input);
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
    let mut other = capture();
    other.time += Duration::from_secs(1);
    other.screen.cells[0] = TextCell {
        character: 0xdb,
        attribute: 4,
    };
    let other = Arc::new(other);
    let mut encoder = device.create_command_encoder(&Default::default());
    let frame = renderer.encode(&queue, &mut encoder, &other);
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
