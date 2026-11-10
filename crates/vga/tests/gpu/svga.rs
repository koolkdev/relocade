use super::{common, pixel, read_pixels};
use common::support::{planar16, registers::palette, svga};
use relocade_clock_test_support::ManualClock;
use relocade_frontend::display::DisplayRenderer;
use relocade_machine::Device;
use relocade_vga::{Vga, VgaDisplay, VgaRenderer};

async fn check_indexed_resolutions() {
    let adapter = wgpu::Instance::default()
        .request_adapter(&Default::default())
        .await
        .unwrap();
    let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
    let mut renderer = VgaRenderer::new(&device).unwrap();
    for (width, height) in [(800, 600), (1024, 768)] {
        let mut vga = Vga::with_clock(ManualClock::new().clock());
        svga::set_mode(&mut vga, width, height, 8);
        palette(&mut vga, 0, [0, 0, 0]);
        palette(&mut vga, 1, [63, 0, 0]);
        let [width, height] = [u32::from(width), u32::from(height)];
        let edges = [0, width - 1, width, width * height - 1];
        for address in edges {
            svga::write_byte(&mut vga, address, 1);
        }
        let capture = vga.snapshot();
        drop(vga);
        renderer.upload(&queue, &capture);
        let mut encoder = device.create_command_encoder(&Default::default());
        let image = renderer.encode(&mut encoder).unwrap();
        queue.submit([encoder.finish()]);
        assert_eq!(image.size(), [width, height]);
        let pixels = read_pixels(&device, &queue, image.texture()).await;
        for (offset, pixel) in pixels.as_chunks::<4>().0.iter().enumerate() {
            assert_eq!(
                *pixel,
                if edges.contains(&(offset as u32)) {
                    [255, 0, 0, 255]
                } else {
                    [0, 0, 0, 255]
                }
            );
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn indexed_resolutions_render_their_full_extent() {
    pollster::block_on(check_indexed_resolutions());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn indexed_resolutions_render_their_full_extent() {
    check_indexed_resolutions().await;
}

async fn check_virtual_layout() {
    let adapter = wgpu::Instance::default()
        .request_adapter(&Default::default())
        .await
        .unwrap();
    let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
    let mut renderer = VgaRenderer::new(&device).unwrap();
    let mut vga = Vga::with_clock(ManualClock::new().clock());
    svga::enable(&mut vga);
    palette(&mut vga, 0, [0, 0, 0]);
    palette(&mut vga, 1, [63, 0, 0]);
    palette(&mut vga, 2, [0, 63, 0]);
    svga::set(&mut vga, 6, 1024);
    svga::set(&mut vga, 8, 384);
    svga::set(&mut vga, 9, 3616);
    let start = 3616 * 1024 + 384;
    for address in [start, start + 639, start + 1024, 4194303] {
        svga::write_byte(&mut vga, address, 1);
    }
    svga::write_byte(&mut vga, start - 1, 2); // Row padding must not enter the image.
    let corner = vga.snapshot();
    svga::set(&mut vga, 8, 0);
    svga::set(&mut vga, 9, 0);
    let origin = vga.snapshot();
    drop(vga);
    for (capture, visible) in [(&corner, true), (&origin, false), (&corner, true)] {
        renderer.upload(&queue, capture);
        let mut encoder = device.create_command_encoder(&Default::default());
        let image = renderer.encode(&mut encoder).unwrap();
        queue.submit([encoder.finish()]);
        let pixels = read_pixels(&device, &queue, image.texture()).await;
        for (offset, pixel) in pixels.as_chunks::<4>().0.iter().enumerate() {
            assert_eq!(
                *pixel,
                if visible && [0, 639, 640, 307199].contains(&offset) {
                    [255, 0, 0, 255]
                } else {
                    [0, 0, 0, 255]
                }
            );
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn virtual_layout_renders_pitch_origin_and_the_last_vram_byte() {
    pollster::block_on(check_virtual_layout());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn virtual_layout_renders_pitch_origin_and_the_last_vram_byte() {
    check_virtual_layout().await;
}

async fn check_packed_pixels() {
    let instance = wgpu::Instance::default();
    let adapter = instance.request_adapter(&Default::default()).await.unwrap();
    let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
    let mut renderer = VgaRenderer::new(&device).unwrap();
    let mut vga = Vga::with_clock(ManualClock::new().clock());
    svga::enable(&mut vga);
    for (index, rgb) in [
        (0, [0, 0, 0]),
        (1, [63, 0, 0]),
        (2, [0, 63, 0]),
        (3, [0, 0, 63]),
        (4, [63, 63, 0]),
        (5, [63, 0, 63]),
        (6, [0, 63, 63]),
        (7, [21, 42, 63]),
    ] {
        palette(&mut vga, index, rgb);
    }
    for (address, index) in [
        (0, 129),
        (639, 2),
        (640, 3),
        (0xffff, 4),
        (0x10000, 5),
        (0x3ffff, 6),
        (0x40000, 7),
        (307199, 1),
    ] {
        svga::write_byte(&mut vga, address, index);
    }
    vga.write_port(0x3c6, 1, 0x7f); // High palette indices obey the shared DAC mask.
    let original = vga.snapshot();
    assert!(renderer.upload(&queue, &original));

    // Legacy reset, blanking, geometry, panning and CPU banking do not select pixels.
    for (port, value) in [
        (0x3c4, 0x0000),
        (0x3c4, 0x2001),
        (0x3d4, 0x0011),
        (0x3d4, 0x0301),
        (0x3d4, 0xff0c),
        (0x3ce, 0x6005),
    ] {
        vga.write_port(port, 2, value);
    }
    vga.write_port(0x3c0, 1, 0);
    svga::set(&mut vga, 5, 63);
    vga.write_port(0x3c8, 1, 200); // DAC transfer position is not a display input.
    let unchanged = vga.snapshot();
    assert!(!renderer.upload(&queue, &unchanged));
    palette(&mut vga, 1, [0, 0, 63]);
    let recolored = vga.snapshot();
    svga::write_byte(&mut vga, 0x40000, 0);
    let cleared = vga.snapshot();
    svga::set(&mut vga, 4, 0);
    drop(vga);

    for (capture, edge, extended) in [
        (&cleared, [0, 0, 255, 255], [0, 0, 0, 255]),
        (&original, [255, 0, 0, 255], [85, 170, 255, 255]),
        (&recolored, [0, 0, 255, 255], [85, 170, 255, 255]),
    ] {
        renderer.upload(&queue, capture);
        let mut encoder = device.create_command_encoder(&Default::default());
        let frame = renderer.encode(&mut encoder).unwrap();
        queue.submit([encoder.finish()]);
        assert_eq!(frame.size(), [640, 480]);
        assert_eq!(frame.pixel_aspect_ratio(), (1, 1));
        let pixels = read_pixels(&device, &queue, frame.texture()).await;
        assert_eq!(pixel(&pixels, 0, 0), edge);
        assert_eq!(pixel(&pixels, 639, 0), [0, 255, 0, 255]);
        assert_eq!(pixel(&pixels, 0, 1), [0, 0, 255, 255]);
        assert_eq!(pixel(&pixels, 255, 102), [255, 255, 0, 255]);
        assert_eq!(pixel(&pixels, 256, 102), [255, 0, 255, 255]);
        assert_eq!(pixel(&pixels, 383, 409), [0, 255, 255, 255]);
        assert_eq!(pixel(&pixels, 384, 409), extended);
        assert_eq!(pixel(&pixels, 639, 479), edge);
        assert_eq!(pixel(&pixels, 638, 479), [0, 0, 0, 255]);
    }

    // The uploaded scanout selects the decoder even when geometry and metadata
    // are identical. VGA also captures fewer VRAM blocks than packed scanout.
    let mut vga = Vga::with_clock(ManualClock::new().clock());
    planar16::program(&mut vga);
    for (index, rgb) in [(0, [0, 0, 0]), (1, [0, 63, 0]), (2, [63, 0, 0])] {
        palette(&mut vga, index, rgb);
    }
    planar16::write_pixels(&mut vga, &[2, 1, 0, 0, 0, 0, 0, 0]);
    let legacy = vga.snapshot();
    drop(vga);
    assert_eq!(legacy.geometry(), original.geometry());

    for (capture, second, extended, last) in [
        (&legacy, [0, 255, 0, 255], [0, 0, 0, 255], [0, 0, 0, 255]),
        (
            &original,
            [0, 0, 0, 255],
            [85, 170, 255, 255],
            [255, 0, 0, 255],
        ),
        (&legacy, [0, 255, 0, 255], [0, 0, 0, 255], [0, 0, 0, 255]),
    ] {
        assert!(renderer.upload(&queue, capture));
        assert!(!renderer.upload(&queue, capture));
        let mut encoder = device.create_command_encoder(&Default::default());
        let frame = renderer.encode(&mut encoder).unwrap();
        queue.submit([encoder.finish()]);
        let pixels = read_pixels(&device, &queue, frame.texture()).await;
        assert_eq!(pixel(&pixels, 0, 0), [255, 0, 0, 255]);
        assert_eq!(pixel(&pixels, 1, 0), second);
        assert_eq!(pixel(&pixels, 384, 409), extended);
        assert_eq!(pixel(&pixels, 639, 479), last);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn packed_pixels_palette_and_retained_captures_render_independently_of_vga_controls() {
    pollster::block_on(check_packed_pixels());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn packed_pixels_palette_and_retained_captures_render_independently_of_vga_controls() {
    check_packed_pixels().await;
}

async fn check_published_display() {
    let instance = wgpu::Instance::default();
    let adapter = instance.request_adapter(&Default::default()).await.unwrap();
    let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
    let clock = ManualClock::default();
    let (vga, mut video) = common::start_video(common::text_vga(clock.clock()));
    common::crtc(&mut *vga.borrow_mut(), 0x0a, 0x20);
    common::fixture::set_cell(&mut *vga.borrow_mut(), 0, 0xdb, 4);
    let source = vga.borrow().frames();
    let mut display = VgaDisplay::new(&device, source.clone()).unwrap();
    let entered = common::boundary(1);
    clock.advance_to(clock.at(entered));
    vga.borrow_mut().read_port(0x3da, 1);
    common::service_video(video.as_mut());
    assert_eq!(display.service(&queue).await, Some(true));
    let mut encoder = device.create_command_encoder(&Default::default());
    let legacy = display.encode_image(&queue, &mut encoder).unwrap().clone();
    queue.submit([encoder.finish()]);
    assert_eq!(legacy.size(), [640, 200]);

    svga::enable(&mut *vga.borrow_mut());
    palette(&mut *vga.borrow_mut(), 6, [42, 21, 0]);
    svga::write_byte(&mut *vga.borrow_mut(), 0, 4);
    svga::write_byte(&mut *vga.borrow_mut(), 0x40000, 1);
    clock.advance_to(clock.at(entered + common::svga_boundary(1)));
    vga.borrow_mut().read_port(0x3da, 1);
    common::service_video(video.as_mut());
    // Let two more publications pass without display service or GPU work.
    svga::write_byte(&mut *vga.borrow_mut(), 0x40000, 5);
    clock.advance_to(clock.at(entered + common::svga_boundary(2)));
    vga.borrow_mut().read_port(0x3da, 1);
    common::service_video(video.as_mut());
    svga::write_byte(&mut *vga.borrow_mut(), 307199, 6);
    clock.advance_to(clock.at(entered + common::svga_boundary(3)));
    vga.borrow_mut().read_port(0x3da, 1);
    common::service_video(video.as_mut());

    assert_eq!(display.service(&queue).await, Some(true));
    let mut encoder = device.create_command_encoder(&Default::default());
    let packed = display.encode_image(&queue, &mut encoder).unwrap().clone();
    queue.submit([encoder.finish()]);
    let pixels = read_pixels(&device, &queue, packed.texture()).await;
    assert_eq!(packed.size(), [640, 480]);
    assert_eq!(pixel(&pixels, 0, 0), [170, 0, 0, 255]);
    assert_eq!(pixel(&pixels, 384, 409), [170, 0, 170, 255]);
    assert_eq!(pixel(&pixels, 639, 479), [170, 85, 0, 255]);

    // New GPU resources reconstruct the latest complete packed state after the
    // producer is gone, even though the first renderer consumed its dirty hints.
    drop(video);
    drop(vga);
    assert_eq!(display.service(&queue).await, None);
    display.device_lost();
    display.attach_device(&device, &queue).unwrap();
    let mut encoder = device.create_command_encoder(&Default::default());
    let restored = display.encode_image(&queue, &mut encoder).unwrap().clone();
    queue.submit([encoder.finish()]);
    assert_eq!(
        read_pixels(&device, &queue, restored.texture()).await,
        pixels
    );
    let legacy_pixels = read_pixels(&device, &queue, legacy.texture()).await;
    assert_eq!(pixel(&legacy_pixels, 0, 0), [170, 0, 0, 255]);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn display_connection_handles_skipped_packed_captures_and_recovers_after_device_drop() {
    pollster::block_on(check_published_display());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn display_connection_handles_skipped_packed_captures_and_recovers_after_device_drop() {
    check_published_display().await;
}

async fn check_direct_color() {
    let adapter = wgpu::Instance::default()
        .request_adapter(&Default::default())
        .await
        .unwrap();
    let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
    let mut renderer = VgaRenderer::new(&device).unwrap();
    for (bits, pitch, rows, values, colors) in [
        (
            16,
            2048,
            2048,
            [0x001fu32, 0x8410, 0x07e0, 0xf800],
            [
                [0, 0, 255, 255],
                [132, 130, 132, 255],
                [0, 255, 0, 255],
                [255, 0, 0, 255],
            ],
        ),
        (
            32,
            4096,
            1024,
            [0x0000_00ff, 0x0033_2211, 0xa533_2211, 0xa5ff_0000],
            [
                [0, 0, 255, 255],
                [51, 34, 17, 255],
                [51, 34, 17, 255],
                [255, 0, 0, 255],
            ],
        ),
    ] {
        let mut vga = Vga::with_clock(ManualClock::new().clock());
        svga::set_mode(&mut vga, 640, 480, bits);
        svga::set(&mut vga, 6, 1024);
        svga::set(&mut vga, 8, 384);
        svga::set(&mut vga, 9, rows - 480);
        let pixel_bytes = u32::from(bits / 8);
        let start = u32::from(rows - 480) * pitch + 384 * pixel_bytes;
        let boundary_row = 65536 / pitch;
        let points = [
            (0, 0, 0),
            (1, 0, 1),
            (639, boundary_row - 1, 2),
            (0, boundary_row, 1),
            (639, 479, 3),
        ];
        for (x, y, color) in points {
            let address = start + y * pitch + x * pixel_bytes;
            for (byte, value) in values[color].to_le_bytes()[..pixel_bytes as usize]
                .iter()
                .copied()
                .enumerate()
            {
                svga::write_byte(&mut vga, address + byte as u32, value);
            }
        }
        let original = vga.snapshot();
        assert_eq!(
            original.vram_byte(4 * 1024 * 1024 - 1),
            Some(if bits == 16 { 0xf8 } else { 0xa5 })
        );
        assert!(renderer.upload(&queue, &original));
        palette(&mut vga, 0, [63, 63, 63]);
        vga.write_port(0x3c6, 1, 0);
        let recolored = vga.snapshot();
        assert!(!renderer.upload(&queue, &recolored));
        svga::set(&mut vga, 8, 0);
        svga::set(&mut vga, 9, 0);
        let panned = vga.snapshot();
        svga::enable(&mut vga);
        svga::write_byte(&mut vga, 0, 1);
        let indexed = vga.snapshot();
        drop(vga);
        // The indexed capture must upload the DAC changes skipped in direct color.
        for (capture, visible, background) in [
            (&recolored, true, [0, 0, 0, 255]),
            (&panned, false, [0, 0, 0, 255]),
            (&original, true, [0, 0, 0, 255]),
            (&indexed, false, [255; 4]),
        ] {
            renderer.upload(&queue, capture);
            let mut encoder = device.create_command_encoder(&Default::default());
            let image = renderer.encode(&mut encoder).unwrap();
            queue.submit([encoder.finish()]);
            assert_eq!(image.size(), [640, 480]);
            let pixels = read_pixels(&device, &queue, image.texture()).await;
            for (index, actual) in pixels.as_chunks::<4>().0.iter().enumerate() {
                let expected = points
                    .iter()
                    .find(|&&(x, y, _)| visible && y * 640 + x == index as u32)
                    .map_or(background, |&(_, _, color)| colors[color]);
                assert_eq!(*actual, expected, "{bits}-bit pixel {index}");
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn direct_color_pixels_pan_and_retain_without_palette_or_alpha_effects() {
    pollster::block_on(check_direct_color());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn direct_color_pixels_pan_and_retain_without_palette_or_alpha_effects() {
    check_direct_color().await;
}
