use super::{common, pixel, read_pixels};
use common::support::{planar16, registers::palette, svga};
use relocade_frontend::display::DisplayRenderer;
use relocade_machine::Device;
use relocade_vga::{Vga, VgaDisplay, VgaRenderer};
use std::rc::Rc;

async fn check_indexed_resolutions() {
    let adapter = wgpu::Instance::default()
        .request_adapter(&Default::default())
        .await
        .unwrap();
    let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
    let mut renderer = VgaRenderer::new(&device).unwrap();
    for (width, height) in [(800, 600), (1024, 768)] {
        let mut vga = Vga::with_clock(Rc::new(common::ManualClock::default()));
        svga::set_mode(&mut vga, width, height);
        palette(&mut vga, 0, [0, 0, 0]);
        palette(&mut vga, 1, [63, 0, 0]);
        let [width, height] = [u32::from(width), u32::from(height)];
        let edges = [0, width - 1, width, width * height - 1];
        for address in edges {
            svga::write_byte(&mut vga, address, 1);
        }
        let capture = common::fixture::capture(&vga);
        drop(vga);
        renderer.upload(&queue, &capture);
        let mut encoder = device.create_command_encoder(&Default::default());
        let image = renderer.encode(&mut encoder).unwrap().image;
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
    let mut vga = Vga::with_clock(Rc::new(common::ManualClock::default()));
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
    let corner = common::fixture::capture(&vga);
    svga::set(&mut vga, 8, 0);
    svga::set(&mut vga, 9, 0);
    let origin = common::fixture::capture(&vga);
    drop(vga);
    for (capture, visible) in [(&corner, true), (&origin, false), (&corner, true)] {
        renderer.upload(&queue, capture);
        let mut encoder = device.create_command_encoder(&Default::default());
        let image = renderer.encode(&mut encoder).unwrap().image;
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
    let mut vga = Vga::with_clock(Rc::new(common::ManualClock::default()));
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
    let original = common::fixture::capture(&vga);
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
    let unchanged = common::fixture::capture(&vga);
    assert!(!renderer.upload(&queue, &unchanged));
    palette(&mut vga, 1, [0, 0, 63]);
    let recolored = common::fixture::capture(&vga);
    svga::write_byte(&mut vga, 0x40000, 0);
    let cleared = common::fixture::capture(&vga);
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
        assert_eq!(frame.image.size(), [640, 480]);
        assert_eq!(frame.image.pixel_aspect_ratio(), (1, 1));
        let pixels = read_pixels(&device, &queue, frame.image.texture()).await;
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
    let mut vga = Vga::with_clock(Rc::new(common::ManualClock::default()));
    planar16::program(&mut vga);
    for (index, rgb) in [(0, [0, 0, 0]), (1, [0, 63, 0]), (2, [63, 0, 0])] {
        palette(&mut vga, index, rgb);
    }
    planar16::write_pixels(&mut vga, &[2, 1, 0, 0, 0, 0, 0, 0]);
    let legacy = common::fixture::capture(&vga);
    drop(vga);
    assert_eq!(legacy.display.geometry(), original.display.geometry());
    assert_eq!(
        (legacy.time, legacy.sequence),
        (original.time, original.sequence)
    );
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
        let pixels = read_pixels(&device, &queue, frame.image.texture()).await;
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
    let clock = Rc::new(common::ManualClock::default());
    let mut vga = common::text_vga(clock.clone());
    common::crtc(&mut vga, 0x0a, 0x20);
    common::fixture::set_cell(&mut vga, 0, 0xdb, 4);
    let source = vga.frame_source();
    let mut display = VgaDisplay::new(&device, source.clone()).unwrap();
    let entered = common::boundary(1);
    clock.set(entered);
    vga.read_port(0x3da, 1);
    let mut encoder = device.create_command_encoder(&Default::default());
    let legacy = display.encode_image(&queue, &mut encoder).unwrap().clone();
    queue.submit([encoder.finish()]);
    assert_eq!(legacy.size(), [640, 200]);

    svga::enable(&mut vga);
    palette(&mut vga, 6, [42, 21, 0]);
    svga::write_byte(&mut vga, 0, 4);
    svga::write_byte(&mut vga, 0x40000, 1);
    clock.set(entered + common::svga_boundary(1));
    vga.read_port(0x3da, 1);
    // Let two more publications pass without any service or GPU work.
    svga::write_byte(&mut vga, 0x40000, 5);
    clock.set(entered + common::svga_boundary(2));
    vga.read_port(0x3da, 1);
    svga::write_byte(&mut vga, 307199, 6);
    clock.set(entered + common::svga_boundary(3));
    vga.read_port(0x3da, 1);
    assert_eq!(source.current_capture().unwrap().sequence, 4);
    assert_eq!(display.service(&queue).await, Some(true));
    let mut encoder = device.create_command_encoder(&Default::default());
    let packed = display.encode_image(&queue, &mut encoder).unwrap().clone();
    queue.submit([encoder.finish()]);
    let pixels = read_pixels(&device, &queue, packed.texture()).await;
    assert_eq!(packed.size(), [640, 480]);
    assert_eq!(pixel(&pixels, 0, 0), [170, 0, 0, 255]);
    assert_eq!(pixel(&pixels, 384, 409), [170, 0, 170, 255]);
    assert_eq!(pixel(&pixels, 639, 479), [170, 85, 0, 255]);
    assert_eq!(display.current_frame().unwrap().sequence, 4);

    // New GPU resources reconstruct the latest complete packed state after the
    // producer is gone, even though the first renderer consumed its dirty hints.
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
