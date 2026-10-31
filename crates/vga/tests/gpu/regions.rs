//! The retained image preserves pixels not addressed by pending VRAM writes.
use super::{common, read_pixels, set_cell};
use common::support::registers::{attribute, indexed, palette};
use relocade_frontend::display::DisplayRenderer;
use relocade_machine::Device;
use relocade_renderer::Image;
use relocade_vga::{
    registers::{crtc, COLOR_INPUT_STATUS_1_PORT},
    Vga, VgaDisplay, VgaRenderer,
};
use std::rc::Rc;

const MARKER: [u8; 4] = [0, 255, 255, 255];

struct Display {
    device: wgpu::Device,
    queue: wgpu::Queue,
    clock: Rc<common::ManualClock>,
    vga: Vga<common::ManualClock>,
    display: VgaDisplay,
    frame: u64,
}

impl Display {
    async fn new() -> Self {
        let adapter = wgpu::Instance::default()
            .request_adapter(&Default::default())
            .await
            .unwrap();
        let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
        let clock = Rc::new(common::ManualClock::default());
        let mut vga = common::text_vga(clock.clone());
        common::crtc(&mut vga, crtc::CURSOR_START, 0x20);
        let display = VgaDisplay::new(&device, vga.frame_source()).unwrap();
        Self {
            device,
            queue,
            clock,
            vga,
            display,
            frame: 0,
        }
    }

    fn publish(&mut self) {
        self.frame += 1;
        self.clock
            .set(std::time::Duration::from_millis(self.frame * 20));
        self.vga.read_port(COLOR_INPUT_STATUS_1_PORT, 1);
    }

    async fn update(&mut self) -> bool {
        self.publish();
        let changed = self.display.update(&self.queue).await.unwrap();
        self.queue.submit([]);
        changed
    }

    fn draw(&mut self) -> Image {
        let mut encoder = self.device.create_command_encoder(&Default::default());
        let image = self.display.encode_image(&mut encoder).unwrap().clone();
        self.queue.submit([encoder.finish()]);
        image
    }

    fn mark(&self, image: &Image, x: u32, y: u32) {
        let mut destination = image.texture().as_image_copy();
        destination.origin = wgpu::Origin3d { x, y, z: 0 };
        self.queue.write_texture(
            destination,
            &MARKER,
            Default::default(),
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([]);
    }

    async fn pixels(&self, image: &Image) -> Vec<u8> {
        read_pixels(&self.device, &self.queue, image.texture()).await
    }

    async fn expect_full(&mut self) {
        let image = self.draw();
        let capture = self.vga.frame_source().current_capture().unwrap();
        let mut reference = VgaRenderer::new(&self.device).unwrap();
        reference.upload(&self.queue, &capture);
        let mut encoder = self.device.create_command_encoder(&Default::default());
        let full = reference.encode(&mut encoder).unwrap();
        self.queue.submit([encoder.finish()]);
        assert_eq!(image.size(), full.image.size());
        assert_eq!(self.pixels(&image).await, self.pixels(&full.image).await);
    }
}

fn pixel(pixels: &[u8], x: usize, y: usize) -> &[u8] {
    &pixels[(y * 640 + x) * 4..(y * 640 + x + 1) * 4]
}

async fn check_cell_regions() {
    let mut gpu = Display::new().await;
    set_cell(&mut gpu.vga, 0, 0xdb, 4);
    set_cell(&mut gpu.vga, 1, b' ', 2);
    assert!(gpu.update().await);
    let image = gpu.draw();
    gpu.mark(&image, 8, 0);
    gpu.mark(&image, 100, 100);

    set_cell(&mut gpu.vga, 0, 0xdb, 1);
    assert!(gpu.update().await);
    assert!(!gpu.update().await);
    gpu.draw();
    let pixels = gpu.pixels(&image).await;
    assert_eq!(pixel(&pixels, 0, 0), [0, 0, 170, 255]);
    assert_eq!(pixel(&pixels, 8, 0), MARKER, "typing updates one cell");
    assert_eq!(pixel(&pixels, 100, 100), MARKER);

    // Retain both write intervals when presentation is delayed.
    set_cell(&mut gpu.vga, 0, 0xdb, 2);
    assert!(gpu.update().await);
    set_cell(&mut gpu.vga, 1, 0xdb, 4);
    assert!(gpu.update().await);
    assert!(!gpu.update().await);
    gpu.draw();
    let pixels = gpu.pixels(&image).await;
    assert_eq!(pixel(&pixels, 0, 0), [0, 170, 0, 255]);
    assert_eq!(pixel(&pixels, 8, 0), [170, 0, 0, 255]);
    assert_eq!(pixel(&pixels, 100, 100), MARKER);
}

async fn check_programmed_text_regions() {
    let mut gpu = Display::new().await;
    // Nine-dot, sixteen-high text with a nonzero start and padded memory rows.
    indexed(&mut gpu.vga, 0x3c4, 1, 0);
    gpu.vga.write_port(0x3c2, 1, 0x47);
    attribute(&mut gpu.vga, 0x13, 8);
    common::crtc(&mut gpu.vga, crtc::MAXIMUM_SCAN_LINE, 0x4f);
    common::crtc(&mut gpu.vga, crtc::OFFSET, 50);
    common::crtc(&mut gpu.vga, crtc::START_ADDRESS_LOW, 80);
    // Row one, column three: start 80 + row stride 100 + column 3.
    set_cell(&mut gpu.vga, 183, 0xdb, 4);
    gpu.clock.set(std::time::Duration::from_millis(20));
    gpu.vga.read_port(COLOR_INPUT_STATUS_1_PORT, 1);
    assert_eq!(gpu.display.update(&gpu.queue).await, Some(true));
    let image = gpu.draw();
    assert_eq!(image.size(), [720, 400]);
    gpu.mark(&image, 26, 16);
    gpu.mark(&image, 27, 15);
    gpu.mark(&image, 100, 100);

    set_cell(&mut gpu.vga, 183, 0xdb, 1);
    gpu.clock.set(std::time::Duration::from_millis(40));
    gpu.vga.read_port(COLOR_INPUT_STATUS_1_PORT, 1);
    assert_eq!(gpu.display.update(&gpu.queue).await, Some(true));
    gpu.draw();
    let pixels = gpu.pixels(&image).await;
    let at = |x: usize, y: usize| &pixels[(y * 720 + x) * 4..(y * 720 + x + 1) * 4];
    assert_eq!(at(27, 16), [0, 0, 170, 255]);
    assert_eq!(at(26, 16), MARKER);
    assert_eq!(at(27, 15), MARKER);
    assert_eq!(at(100, 100), MARKER);

    // Reuse the shader's nine-dot panning: value 3 shifts the image by four dots.
    attribute(&mut gpu.vga, 0x13, 3);
    gpu.clock.set(std::time::Duration::from_millis(60));
    gpu.vga.read_port(COLOR_INPUT_STATUS_1_PORT, 1);
    assert_eq!(gpu.display.update(&gpu.queue).await, Some(true));
    gpu.draw();
    gpu.mark(&image, 22, 16);
    gpu.mark(&image, 100, 100);
    set_cell(&mut gpu.vga, 183, 0xdb, 4);
    gpu.clock.set(std::time::Duration::from_millis(80));
    gpu.vga.read_port(COLOR_INPUT_STATUS_1_PORT, 1);
    assert_eq!(gpu.display.update(&gpu.queue).await, Some(true));
    gpu.draw();
    let pixels = gpu.pixels(&image).await;
    let at = |x: usize, y: usize| &pixels[(y * 720 + x) * 4..(y * 720 + x + 1) * 4];
    assert_eq!(at(23, 16), [170, 0, 0, 255]);
    assert_eq!(at(22, 16), MARKER);
    assert_eq!(at(100, 100), MARKER);

    // The lower display restarts at address zero after scanline fifteen.
    attribute(&mut gpu.vga, 0x13, 8);
    let overflow = gpu.vga.snapshot().registers.crtc.values[crtc::OVERFLOW as usize];
    common::crtc(&mut gpu.vga, crtc::OVERFLOW, overflow & !0x10);
    common::crtc(&mut gpu.vga, crtc::MAXIMUM_SCAN_LINE, 0x0f);
    common::crtc(&mut gpu.vga, crtc::LINE_COMPARE, 15);
    gpu.clock.set(std::time::Duration::from_millis(100));
    gpu.vga.read_port(COLOR_INPUT_STATUS_1_PORT, 1);
    assert_eq!(gpu.display.update(&gpu.queue).await, Some(true));
    gpu.draw();
    gpu.mark(&image, 27, 0);
    gpu.mark(&image, 100, 100);
    set_cell(&mut gpu.vga, 3, 0xdb, 2);
    gpu.clock.set(std::time::Duration::from_millis(120));
    gpu.vga.read_port(COLOR_INPUT_STATUS_1_PORT, 1);
    assert_eq!(gpu.display.update(&gpu.queue).await, Some(true));
    gpu.draw();
    let pixels = gpu.pixels(&image).await;
    let at = |x: usize, y: usize| &pixels[(y * 720 + x) * 4..(y * 720 + x + 1) * 4];
    assert_eq!(at(27, 16), [0, 170, 0, 255]);
    assert_eq!(at(27, 0), MARKER);
    assert_eq!(at(100, 100), MARKER);
}

async fn check_full_fallbacks() {
    let mut gpu = Display::new().await;
    set_cell(&mut gpu.vga, 0, 0xdb, 4);
    assert!(gpu.update().await);
    let image = gpu.draw();

    // Any state change redraws fully, even if ordinary writes arrive before drawing.
    gpu.mark(&image, 100, 100);
    common::crtc(&mut gpu.vga, crtc::CURSOR_START, 6);
    assert!(gpu.update().await);
    set_cell(&mut gpu.vga, 2, 0xdb, 2);
    assert!(gpu.update().await);
    gpu.expect_full().await;

    gpu.mark(&image, 100, 100);
    gpu.frame = 15;
    assert!(gpu.update().await); // Blink phase, with no guest writes.
    gpu.expect_full().await;

    gpu.mark(&image, 100, 100);
    set_cell(&mut gpu.vga, 1, 0xdb, 2);
    gpu.publish(); // Skip the capture containing these writes.
    assert!(gpu.update().await);
    gpu.expect_full().await;

    gpu.mark(&image, 100, 100);
    palette(&mut gpu.vga, 4, [0, 0, 63]);
    assert!(gpu.update().await);
    gpu.expect_full().await;

    gpu.mark(&image, 100, 100);
    indexed(&mut gpu.vga, 0x3c4, 2, 4);
    indexed(&mut gpu.vga, 0x3c4, 4, 6);
    indexed(&mut gpu.vga, 0x3ce, 5, 0);
    indexed(&mut gpu.vga, 0x3ce, 6, 5);
    gpu.vga.write_mmio(0xa0000 + 0xdb * 32, 1, 0);
    indexed(&mut gpu.vga, 0x3c4, 2, 3);
    indexed(&mut gpu.vga, 0x3c4, 4, 2);
    indexed(&mut gpu.vga, 0x3ce, 5, 0x10);
    indexed(&mut gpu.vga, 0x3ce, 6, 0x0e);
    assert!(gpu.update().await);
    gpu.expect_full().await;

    // An image replacement forces all pixels to be initialized.
    attribute(&mut gpu.vga, 0x13, 0);
    set_cell(&mut gpu.vga, 79, 0xdb, 4);
    assert!(gpu.update().await);
    let retrace_end = gpu.vga.snapshot().registers.crtc.values[crtc::VERTICAL_RETRACE_END as usize];
    common::crtc(&mut gpu.vga, crtc::VERTICAL_RETRACE_END, retrace_end & 0x7f);
    common::crtc(&mut gpu.vga, crtc::HORIZONTAL_DISPLAY_END, 39);
    common::crtc(&mut gpu.vga, crtc::VERTICAL_RETRACE_END, retrace_end);
    assert!(gpu.update().await);
    gpu.expect_full().await;

    common::support::indexed256::program(&mut gpu.vga);
    assert!(gpu.update().await);
    gpu.expect_full().await;
}

async fn check_graphics_regions() {
    let mut gpu = Display::new().await;
    for unchained in [false, true] {
        common::support::indexed256::program(&mut gpu.vga);
        if unchained {
            common::support::indexed256::unchain(&mut gpu.vga);
            indexed(&mut gpu.vga, 0x3c4, 2, 2);
        }
        palette(&mut gpu.vga, 4, [63, 0, 0]);
        let address = if unchained { 0xa0000 } else { 0xa0001 };
        gpu.vga.write_mmio(address, 1, 0);
        assert!(gpu.update().await);
        let image = gpu.draw();
        assert_eq!(image.size(), [320, 200]);
        // Pixels zero and two fetch the same address from other planes.
        gpu.mark(&image, 0, 0);
        gpu.mark(&image, 2, 0);
        gpu.vga.write_mmio(address, 1, 4);
        assert!(gpu.update().await);
        gpu.draw();
        let pixels = gpu.pixels(&image).await;
        assert_eq!(&pixels[4..8], [255, 0, 0, 255]);
        assert_eq!(&pixels[..4], MARKER);
        assert_eq!(&pixels[8..12], MARKER);
    }

    for interleaved in [false, true] {
        common::support::planar16::program(&mut gpu.vga);
        gpu.vga.write_mmio(0xa0000, 1, 0); // Clear this address in every plane.
        indexed(&mut gpu.vga, 0x3c4, 2, 4);
        if interleaved {
            indexed(&mut gpu.vga, 0x3ce, 5, 0x20);
        }
        palette(&mut gpu.vga, if interleaved { 8 } else { 4 }, [63, 0, 0]);
        assert!(gpu.update().await);
        let image = gpu.draw();
        gpu.mark(&image, 8, 0);
        // Plane two is pixel data here; unrelated graphics pixels stay intact.
        gpu.vga.write_mmio(0xa0000, 1, 0x80);
        assert!(gpu.update().await);
        gpu.draw();
        let pixels = gpu.pixels(&image).await;
        assert_eq!(&pixels[..4], [255, 0, 0, 255]);
        assert_eq!(&pixels[32..36], MARKER);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn retained_text_updates_changed_cells() {
    pollster::block_on(check_cell_regions());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn retained_text_updates_changed_cells() {
    check_cell_regions().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn retained_text_updates_follow_programmed_geometry_panning_and_split() {
    pollster::block_on(check_programmed_text_regions());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn retained_text_updates_follow_programmed_geometry_panning_and_split() {
    check_programmed_text_regions().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn retained_image_redraws_for_state_fonts_and_unknown_baselines() {
    pollster::block_on(check_full_fallbacks());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn retained_image_redraws_for_state_fonts_and_unknown_baselines() {
    check_full_fallbacks().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn retained_graphics_updates_filter_physical_plane_addresses() {
    pollster::block_on(check_graphics_regions());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn retained_graphics_updates_filter_physical_plane_addresses() {
    check_graphics_regions().await;
}
