use relocade_renderer::{FrameError, Renderer};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

fn target(device: &wgpu::Device, size: [u32; 2], format: wgpu::TextureFormat) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("pixel test output"),
        size: wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

async fn read_pixels(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
) -> Vec<[u8; 4]> {
    let row_bytes = texture.width() * 4;
    let stride =
        row_bytes.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("pixel test readback"),
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
    let (sender, receiver) = futures_channel::oneshot::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            sender.send(result).unwrap();
        });
    #[cfg(not(target_arch = "wasm32"))]
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    receiver.await.unwrap().unwrap();
    let mapped = buffer.slice(..).get_mapped_range().unwrap();
    let mut result = Vec::new();
    for row in mapped.chunks_exact(stride as usize) {
        for pixel in row[..row_bytes as usize].as_chunks::<4>().0 {
            let mut rgba = *pixel;
            if matches!(
                texture.format(),
                wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
            ) {
                rgba.swap(0, 2);
            }
            result.push(rgba);
        }
    }
    drop(mapped);
    buffer.unmap();
    result
}

fn assert_color(actual: [u8; 4], expected: [u8; 4]) {
    // Allow a one-unit rounding difference in the sRGB decode/encode round trip.
    for (actual, expected) in actual.into_iter().zip(expected) {
        assert!(actual.abs_diff(expected) <= 1, "{actual} != {expected}");
    }
}

async fn check_pixels() {
    let instance = wgpu::Instance::default();
    let adapter = instance.request_adapter(&Default::default()).await.unwrap();
    let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
    for format in [
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureFormat::Rgba8UnormSrgb,
        wgpu::TextureFormat::Bgra8Unorm,
        wgpu::TextureFormat::Bgra8UnormSrgb,
    ] {
        let mut renderer = Renderer::new(device.clone(), queue.clone(), format).unwrap();
        let output = target(&device, [4, 4], format);
        let view = output.create_view(&Default::default());
        let colors: [[u8; 4]; 4] = [
            [170, 85, 0, 255],
            [32, 64, 96, 255],
            [0, 170, 85, 255],
            [85, 0, 170, 255],
        ];
        renderer
            .render([2, 2], colors.as_flattened(), &view)
            .unwrap();
        let pixels = read_pixels(&device, &queue, &output).await;
        for y in 0..4 {
            for x in 0..4 {
                assert_color(pixels[y * 4 + x], colors[(y / 2) * 2 + x / 2]);
            }
        }
        // The next frame updates the existing texture, including alpha.
        let changed: [[u8; 4]; 4] = [[32, 64, 96, 17]; 4];
        renderer
            .render([2, 2], changed.as_flattened(), &view)
            .unwrap();
        for pixel in read_pixels(&device, &queue, &output).await {
            assert_color(pixel, changed[0]);
        }
        assert_eq!(
            renderer.render([0, 2], &[], &view),
            Err(FrameError::InvalidSize)
        );
        assert_eq!(
            renderer.render([u32::MAX, 1], &[], &view),
            Err(FrameError::InvalidSize)
        );
        assert_eq!(
            renderer.render([2, 2], &[0; 15], &view),
            Err(FrameError::InvalidLength {
                expected: 16,
                actual: 15
            })
        );
        // Resize both source and output, with an unpadded 12-byte upload row.
        let output = target(&device, [6, 2], format);
        renderer
            .render(
                [3, 1],
                colors[..3].as_flattened(),
                &output.create_view(&Default::default()),
            )
            .unwrap();
        for (index, pixel) in read_pixels(&device, &queue, &output)
            .await
            .into_iter()
            .enumerate()
        {
            assert_color(pixel, colors[(index % 6) / 2]);
        }
    }
    assert!(matches!(
        Renderer::new(
            device.clone(),
            queue.clone(),
            wgpu::TextureFormat::Rgba16Float
        ),
        Err(FrameError::UnsupportedFormat(
            wgpu::TextureFormat::Rgba16Float
        ))
    ));
    #[cfg(not(target_arch = "wasm32"))]
    check_text_screen(&device, &queue).await;
}

#[cfg(not(target_arch = "wasm32"))]
async fn check_text_screen(device: &wgpu::Device, queue: &wgpu::Queue) {
    use relocade_machine::{Clock, Device};
    use relocade_vga::{Vga, TEXT_MEMORY_START};
    use std::{cell::Cell, rc::Rc, time::Duration};
    struct TestClock(Cell<Duration>);
    impl Clock for TestClock {
        fn now(&self) -> Duration {
            self.0.get()
        }
        async fn wait_until(&self, _: Duration) {
            std::future::pending().await
        }
    }
    let clock = Rc::new(TestClock(Cell::new(Duration::ZERO)));
    let mut vga = Vga::with_clock(clock.clone());
    for cell in 0..2000 {
        vga.write_mmio(TEXT_MEMORY_START + cell * 2, 2, 0x0720);
    }
    vga.write_mmio(TEXT_MEMORY_START, 2, 0x1edb);
    for value in [0x070e, 0xcf0f, 0x060a, 0x070b] {
        vga.write_port(0x3d4, 2, value);
    }
    clock.0.set(Duration::from_millis(20));
    vga.read_port(0x3da, 1);
    let frame = vga.frame_source().current_capture().unwrap().rasterize();
    let size = [frame.width, frame.height];
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let output = target(device, size, format);
    let mut renderer = Renderer::new(device.clone(), queue.clone(), format).unwrap();
    renderer
        .render(
            size,
            &frame.pixels,
            &output.create_view(&Default::default()),
        )
        .unwrap();
    let pixels = read_pixels(device, queue, &output).await;
    assert_color(pixels[0], [255, 255, 85, 255]);
    assert_color(pixels[8], [0, 0, 0, 255]);
    assert_color(pixels[640 * 200 - 1], [170, 170, 170, 255]);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn uploads_scales_and_resizes_pixel_frames() {
    pollster::block_on(check_pixels());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn uploads_scales_and_resizes_pixel_frames() {
    check_pixels().await;
}
