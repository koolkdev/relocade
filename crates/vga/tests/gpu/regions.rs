//! The display adapter preserves pixels outside each submitted update rectangle.
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
        self.clock.set(common::boundary(self.frame));
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

    attribute(&mut gpu.vga, 0x13, 1); // Panned text uses the full decoder.
    assert!(gpu.update().await);
    gpu.expect_full().await;
    gpu.mark(&image, 100, 100);
    set_cell(&mut gpu.vga, 0, 0xdb, 2);
    assert!(gpu.update().await);
    gpu.expect_full().await;

    // An image replacement drops rectangles accumulated for the larger image.
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
    let graphics = gpu.draw();
    gpu.mark(&graphics, 100, 100);
    gpu.vga.write_mmio(0xa0000, 1, 4);
    assert!(gpu.update().await);
    gpu.expect_full().await;
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
fn retained_text_regions_follow_programmed_start_stride_and_cell_size() {
    pollster::block_on(check_programmed_text_regions());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn retained_text_regions_follow_programmed_start_stride_and_cell_size() {
    check_programmed_text_regions().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn retained_text_restores_full_image_for_uncertain_or_global_changes() {
    pollster::block_on(check_full_fallbacks());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn retained_text_restores_full_image_for_uncertain_or_global_changes() {
    check_full_fallbacks().await;
}
