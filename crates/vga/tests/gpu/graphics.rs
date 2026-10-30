//! GPU comparison and distinctive palette values for graphics-layout checks.
use super::{common::support::registers::palette, read_pixels};
use relocade_machine::Device;
use relocade_vga::{CapturedFrame, VgaRenderer};

pub(super) fn color(index: u8) -> ([u8; 3], [u8; 4]) {
    // Eight exact six-bit levels and their independently listed RGBA values.
    let dac = [0, 9, 18, 27, 36, 45, 54, 63];
    let rgba = [0, 36, 73, 109, 146, 182, 219, 255];
    let r = usize::from(index & 7);
    let g = usize::from((index >> 3) & 7);
    let b = usize::from(index >> 6);
    (
        [dac[r], dac[g], [0, 21, 42, 63][b]],
        [rgba[r], rgba[g], [0, 85, 170, 255][b], 255],
    )
}

pub(super) fn load_palette(vga: &mut impl Device) {
    for index in 0..=255 {
        palette(vga, index, color(index).0);
    }
}

pub(super) fn pixels(indices: impl IntoIterator<Item = u8>) -> Vec<u8> {
    indices
        .into_iter()
        .flat_map(|index| color(index).1)
        .collect()
}

pub(super) struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: VgaRenderer,
}

impl Gpu {
    pub(super) async fn new() -> Self {
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

    pub(super) async fn expect(
        &mut self,
        capture: &CapturedFrame,
        size: [u32; 2],
        expected: &[u8],
    ) {
        self.renderer.upload(&self.queue, capture);
        let mut encoder = self.device.create_command_encoder(&Default::default());
        let frame = self.renderer.encode(&mut encoder).unwrap();
        self.queue.submit([encoder.finish()]);
        assert_eq!(frame.image.size(), size);
        let actual = read_pixels(&self.device, &self.queue, frame.image.texture()).await;
        assert_eq!(actual.len(), expected.len());
        for (index, (actual, expected)) in actual
            .as_chunks::<4>()
            .0
            .iter()
            .zip(expected.as_chunks::<4>().0)
            .enumerate()
        {
            assert_eq!(
                actual,
                expected,
                "pixel ({}, {})",
                index % size[0] as usize,
                index / size[0] as usize
            );
        }
    }
}
