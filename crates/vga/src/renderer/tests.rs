use super::*;
use crate::{memory::VideoMemory, DacSnapshot, PackedLayout, PixelFormat};

#[path = "../../tests/gpu/readback.rs"]
mod readback;

async fn bgr888_captures() {
    let adapter = wgpu::Instance::default()
        .request_adapter(&Default::default())
        .await
        .unwrap();
    let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
    let mut renderer = VgaRenderer::new(&device).unwrap();
    for wrapping in [false, true] {
        let mut memory = VideoMemory::new(if wrapping { 65536 } else { 131072 });
        let layout = if wrapping {
            PackedLayout::wrapping([4, 2], PixelFormat::Bgr888, 15, 65535, 65536)
        } else {
            PackedLayout::new([4, 2], PixelFormat::Bgr888, 15, 65535, 131072)
        }
        .unwrap();
        let mask = if wrapping { 0xffff } else { u32::MAX };
        // All four word alignments, a 64-KiB boundary and padded scanlines.
        for (row, bytes) in [
            [51, 34, 17, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255],
            [
                3, 2, 1, 254, 127, 63, 11, 22, 33, 100, 150, 200, 255, 255, 255,
            ],
        ]
        .into_iter()
        .enumerate()
        {
            for (byte, value) in bytes.into_iter().enumerate() {
                memory.write_physical((65535 + row as u32 * 15 + byte as u32) & mask, value);
            }
        }
        let original = DisplaySnapshot {
            memory: memory.snapshot(memory.capacity(), None),
            dac: DacSnapshot {
                colors: [[63; 3]; 256],
                mask: 0,
                bits_per_component: 6,
            },
            scanout: Scanout::Packed(layout),
            blanked: false,
            blanking: Default::default(),
            changes: None,
        };
        memory.clear_vram();
        let cleared = DisplaySnapshot {
            memory: memory.snapshot(memory.capacity(), None),
            ..original.clone()
        };
        drop(memory);
        for (capture, cleared) in [(&original, false), (&cleared, true), (&original, false)] {
            renderer.upload(&queue, capture);
            let mut encoder = device.create_command_encoder(&Default::default());
            let image = renderer.encode(&mut encoder).unwrap();
            queue.submit([encoder.finish()]);
            assert_eq!(image.size(), [4, 2]);
            let pixels = readback::read_pixels(&device, &queue, image.texture()).await;
            let expected = if cleared {
                [[0, 0, 0, 255]; 8]
            } else {
                [
                    [17, 34, 51, 255],
                    [255, 0, 0, 255],
                    [0, 255, 0, 255],
                    [0, 0, 255, 255],
                    [1, 2, 3, 255],
                    [63, 127, 254, 255],
                    [33, 22, 11, 255],
                    [200, 150, 100, 255],
                ]
            };
            assert_eq!(pixels, expected.as_flattened());
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn bgr888_captures_render_unaligned_pixels_padding_and_memory_wraps() {
    pollster::block_on(bgr888_captures());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn bgr888_captures_render_unaligned_pixels_padding_and_memory_wraps() {
    bgr888_captures().await;
}

async fn dac_precision_captures() {
    let adapter = wgpu::Instance::default()
        .request_adapter(&Default::default())
        .await
        .unwrap();
    let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
    let mut renderer = VgaRenderer::new(&device).unwrap();
    let mut memory = VideoMemory::new(65536);
    memory.write_physical(0, 0x81);
    let mut colors = [[0; 3]; 256];
    colors[1] = [63, 21, 7];
    let six = DisplaySnapshot {
        memory: memory.snapshot(memory.capacity(), None),
        dac: DacSnapshot {
            colors,
            mask: 1,
            bits_per_component: 6,
        },
        scanout: Scanout::Packed(
            PackedLayout::new([1, 1], PixelFormat::Indexed8, 1, 0, 65536).unwrap(),
        ),
        blanked: false,
        blanking: Default::default(),
        changes: None,
    };
    // Only the precision changes: the renderer must upload new DAC controls even
    // when the palette, index mask and image bytes are identical.
    let mut eight = six.clone();
    eight.dac.bits_per_component = 8;
    let mut full = eight.clone();
    full.dac.colors[1] = [200, 129, 255];
    drop(memory);
    for (capture, expected) in [
        (&six, [255, 85, 28, 255]),
        (&eight, [63, 21, 7, 255]),
        (&full, [200, 129, 255, 255]),
        (&six, [255, 85, 28, 255]),
    ] {
        renderer.upload(&queue, capture);
        let mut encoder = device.create_command_encoder(&Default::default());
        let image = renderer.encode(&mut encoder).unwrap();
        queue.submit([encoder.finish()]);
        assert_eq!(
            readback::read_pixels(&device, &queue, image.texture()).await,
            expected
        );
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn dac_precision_and_palette_are_owned_by_each_capture() {
    pollster::block_on(dac_precision_captures());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn dac_precision_and_palette_are_owned_by_each_capture() {
    dac_precision_captures().await;
}
