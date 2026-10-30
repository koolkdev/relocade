//! Guest BIOS calls, MMIO and DAC writes feed the ordinary captured GPU path.
#![cfg(not(target_arch = "wasm32"))]

#[path = "video_gpu/characters.rs"]
mod characters;
mod common;
#[path = "video_gpu/fonts.rs"]
mod fonts;
#[path = "video_gpu/indexed256.rs"]
mod indexed256;
#[path = "video_gpu/pages.rs"]
mod pages;
#[path = "video_gpu/palette.rs"]
mod palette;
#[path = "video_gpu/pixels.rs"]
mod pixels;
#[path = "video_gpu/planar16.rs"]
mod planar16;
#[path = "../../vga/tests/gpu/readback.rs"]
mod readback;
#[path = "video_gpu/transitions.rs"]
mod transitions;
#[path = "video_gpu/unchained256.rs"]
mod unchained256;

use relocade_dos::Dos;
use relocade_vga::{CapturedFrame, VgaRenderer};
use relocade_x86::{ExecutionExit, HostExit};

async fn run(dos: &mut Dos, program: &[u8]) -> std::sync::Arc<CapturedFrame> {
    dos.launch_com(program, Default::default()).unwrap();
    assert_eq!(
        dos.cpu().run(100_000).await,
        ExecutionExit::Host(HostExit::Return)
    );
    common::next_capture(dos).await
}

// The harness owns GPU resource setup and readback for each guest interaction.
struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: VgaRenderer,
}

impl Gpu {
    async fn new() -> Self {
        let adapter = wgpu::Instance::default()
            .request_adapter(&Default::default())
            .await
            .unwrap();
        let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
        let renderer = VgaRenderer::new(&device).unwrap();
        Self {
            device,
            queue,
            renderer,
        }
    }

    async fn render(&mut self, capture: &CapturedFrame, size: [u32; 2]) -> Vec<u8> {
        self.renderer.upload(&self.queue, capture);
        let mut encoder = self.device.create_command_encoder(&Default::default());
        let image = self.renderer.encode(&mut encoder).unwrap().image;
        self.queue.submit([encoder.finish()]);
        assert_eq!(image.size(), size);
        readback::read_pixels(&self.device, &self.queue, image.texture()).await
    }
}

const TEXT_WITH_RED_BLOCK: &[u8] = &[
    0xb8, 3, 0, 0xcd, 0x10, // Return to mode 3; restore the font.
    0xb8, 0, 1, 0xb9, 7, 0x20, 0xcd, 0x10, // Hide cursor.
    0xb8, 0, 0xb8, 0x8e, 0xc0, // ES = B800.
    0x26, 0xc7, 0x06, 0, 0, 0xdb, 4, // First cell is a red full block.
    0xc3,
];
