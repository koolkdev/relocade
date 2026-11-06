use super::{common, pixel, read_pixels};
use common::support::{planar16, registers::palette, svga};
use relocade_machine::Device;
use relocade_vga::{Vga, VgaRenderer};
use std::rc::Rc;

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
