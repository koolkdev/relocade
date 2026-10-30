#[cfg(not(target_arch = "wasm32"))]
#[path = "../../vga/tests/support/mod.rs"]
mod vga_support;
#[cfg(not(target_arch = "wasm32"))]
use vga_support::{fixture as vga_fixture, software};

use relocade_renderer::{fit_image, FrameError, Image, Rect, Renderer};

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

// This fixture owns its output pass, placement, clear policy and submission.
fn draw(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    renderer: &mut Renderer,
    image: &Image,
    target: &wgpu::TextureView,
    size: [u32; 2],
) {
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        renderer.draw_image(&mut pass, image, fit_image(image, Rect::from_size(size)));
    }
    queue.submit([encoder.finish()]);
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
        let mut renderer = Renderer::new(device.clone(), format).unwrap();
        let output = target(&device, [4, 4], format);
        let view = output.create_view(&Default::default());
        let colors: [[u8; 4]; 4] = [
            [170, 85, 0, 255],
            [32, 64, 96, 255],
            [0, 170, 85, 255],
            [85, 0, 170, 255],
        ];
        let image = Image::new(&device, [2, 2], (1, 1)).unwrap();
        image.upload(&queue, colors.as_flattened()).unwrap();
        draw(&device, &queue, &mut renderer, &image, &view, [4, 4]);
        let pixels = read_pixels(&device, &queue, &output).await;
        for y in 0..4 {
            for x in 0..4 {
                assert_color(pixels[y * 4 + x], colors[(y / 2) * 2 + x / 2]);
            }
        }
        // Update the same texture, including alpha and the sRGB near-black transition.
        let changed: [[u8; 4]; 4] = [[1, 10, 11, 17]; 4];
        image.upload(&queue, changed.as_flattened()).unwrap();
        draw(&device, &queue, &mut renderer, &image, &view, [4, 4]);
        for pixel in read_pixels(&device, &queue, &output).await {
            assert_color(pixel, changed[0]);
        }
        assert!(matches!(
            Image::new(&device, [0, 2], (1, 1)),
            Err(FrameError::InvalidSize)
        ));
        assert!(matches!(
            Image::new(&device, [u32::MAX, 1], (1, 1)),
            Err(FrameError::InvalidSize)
        ));
        assert!(matches!(
            Image::new(&device, [2, 2], (0, 1)),
            Err(FrameError::InvalidAspectRatio)
        ));
        assert_eq!(
            image.upload(&queue, &[0; 15]),
            Err(FrameError::InvalidLength {
                expected: 16,
                actual: 15
            })
        );
        // Resize both source and output, with an unpadded 12-byte upload row.
        let output = target(&device, [6, 2], format);
        let resized = Image::new(&device, [3, 1], (1, 1)).unwrap();
        resized.upload(&queue, colors[..3].as_flattened()).unwrap();
        draw(
            &device,
            &queue,
            &mut renderer,
            &resized,
            &output.create_view(&Default::default()),
            [6, 2],
        );
        for (index, pixel) in read_pixels(&device, &queue, &output)
            .await
            .into_iter()
            .enumerate()
        {
            assert_color(pixel, colors[(index % 6) / 2]);
        }
    }
    assert!(matches!(
        Renderer::new(device.clone(), wgpu::TextureFormat::Rgba16Float),
        Err(FrameError::UnsupportedFormat(
            wgpu::TextureFormat::Rgba16Float
        ))
    ));
    check_gpu_image_and_placement(&device, &queue).await;
    check_shared_output(&device, &queue).await;
    #[cfg(not(target_arch = "wasm32"))]
    check_text_screen(&device, &queue).await;
}

async fn check_shared_output(device: &wgpu::Device, queue: &wgpu::Queue) {
    let red = Image::new(device, [1, 1], (1, 1)).unwrap();
    red.upload(queue, &[170, 0, 0, 255]).unwrap();
    let green = Image::new(device, [1, 1], (1, 1)).unwrap();
    green.upload(queue, &[0, 170, 0, 255]).unwrap();
    let output = Image::new(device, [12, 8], (1, 1)).unwrap();
    let mut renderer = Renderer::new(device.clone(), Image::FORMAT).unwrap();
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        // The caller initializes the attachment before a separate, preserving pass.
        let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: output.view(),
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.0,
                        g: 0.0,
                        b: 170.0 / 255.0,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
    }
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: output.view(),
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_scissor_rect(2, 1, 8, 6);
        renderer.draw_image(
            &mut pass,
            &red,
            Rect {
                x: 1.0,
                y: 2.0,
                width: 4.0,
                height: 3.0,
            },
        );
        let destination = fit_image(
            &green,
            Rect {
                x: 6.0,
                y: 0.0,
                width: 6.0,
                height: 4.0,
            },
        );
        assert_eq!(
            destination,
            Rect {
                x: 7.0,
                y: 0.0,
                width: 4.0,
                height: 4.0,
            }
        );
        renderer.draw_image(&mut pass, &green, destination);
        renderer.draw_image(&mut pass, &red, Rect::from_size([0, 8]));
    }
    queue.submit([encoder.finish()]);
    let pixels = read_pixels(device, queue, output.texture()).await;
    for y in 0..8 {
        for x in 0..12 {
            let expected = if (2..5).contains(&x) && (2..5).contains(&y) {
                [170, 0, 0, 255]
            } else if (7..10).contains(&x) && (1..4).contains(&y) {
                [0, 170, 0, 255]
            } else {
                [0, 0, 170, 255]
            };
            assert_eq!(pixels[y * 12 + x], expected, "pixel ({x}, {y})");
        }
    }
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
    vga_fixture::initialize_font(&mut vga);
    for cell in 0..2000 {
        vga.write_mmio(TEXT_MEMORY_START + cell * 2, 2, 0x0720);
    }
    vga.write_mmio(TEXT_MEMORY_START, 2, 0x1edb);
    for value in [0x070e, 0xcf0f, 0x060a, 0x070b] {
        vga.write_port(0x3d4, 2, value);
    }
    clock.0.set(Duration::from_millis(20));
    vga.read_port(0x3da, 1);
    let frame = software::rasterize(&vga.frame_source().current_capture().unwrap());
    let size = [frame.width, frame.height];
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let output_size = [640, 480];
    let output = target(device, output_size, format);
    let image = Image::new(device, size, frame.pixel_aspect_ratio).unwrap();
    image.upload(queue, &frame.pixels).unwrap();
    let mut renderer = Renderer::new(device.clone(), format).unwrap();
    draw(
        device,
        queue,
        &mut renderer,
        &image,
        &output.create_view(&Default::default()),
        output_size,
    );
    let pixels = read_pixels(device, queue, &output).await;
    assert_color(pixels[0], [255, 255, 85, 255]);
    assert_color(pixels[8], [0, 0, 0, 255]);
    assert_color(pixels[640 * 480 - 1], [170, 170, 170, 255]);
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

// GPU-created contents must be viewable without a CPU upload. Viewing at a
// different size must preserve both source storage and its display proportions.
async fn check_gpu_image_and_placement(device: &wgpu::Device, queue: &wgpu::Queue) {
    let image = Image::new(device, [640, 200], (5, 12)).unwrap();
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: image.view(),
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 170.0 / 255.0,
                        g: 85.0 / 255.0,
                        b: 0.0,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
    }
    queue.submit([encoder.finish()]);
    for format in [
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureFormat::Bgra8UnormSrgb,
    ] {
        let mut renderer = Renderer::new(device.clone(), format).unwrap();
        for (size, bounds) in [([16, 16], [0, 2, 16, 14]), ([24, 12], [4, 0, 20, 12])] {
            let output = target(device, size, format);
            draw(
                device,
                queue,
                &mut renderer,
                &image,
                &output.create_view(&Default::default()),
                size,
            );
            let pixels = read_pixels(device, queue, &output).await;
            for y in 0..size[1] {
                for x in 0..size[0] {
                    let inside = x >= bounds[0] && x < bounds[2] && y >= bounds[1] && y < bounds[3];
                    assert_color(
                        pixels[(y * size[0] + x) as usize],
                        if inside {
                            [170, 85, 0, 255]
                        } else {
                            [0, 0, 0, 255]
                        },
                    );
                }
            }
        }
    }
    assert_eq!(image.size(), [640, 200]);
    assert_eq!(image.pixel_aspect_ratio(), (5, 12));
    assert_color(
        read_pixels(device, queue, image.texture()).await[0],
        [170, 85, 0, 255],
    );
}
