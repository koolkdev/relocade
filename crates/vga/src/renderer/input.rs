use crate::{text, CapturedFrame, TextScreen};
use wgpu::util::{DeviceExt, TextureDataOrder};

// Eight header words followed by packed character/attribute pairs; matches Text in WGSL.
// Keeping the 4 KB input in a uniform also supports downlevel GPU backends.
const HEADER_BYTES: usize = 32;
const INPUT_BYTES: usize = HEADER_BYTES + TextScreen::COLUMNS * TextScreen::ROWS * 2;

pub(super) struct TextInput {
    buffer: wgpu::Buffer,
    bytes: [u8; INPUT_BYTES],
    pub(super) binding: wgpu::BindGroup,
}

impl TextInput {
    pub(super) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("captured VGA text"),
            size: INPUT_BYTES as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let font = texture(
            device,
            queue,
            [text::GLYPH_HEIGHT as u32, 256],
            wgpu::TextureFormat::R8Uint,
            text::FONT,
        );
        let palette = texture(
            device,
            queue,
            [16, 1],
            wgpu::TextureFormat::Rgba8Unorm,
            text::PALETTE.as_flattened(),
        );
        let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("VGA text resources"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&font),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&palette),
                },
            ],
        });
        Self {
            buffer,
            bytes: [0; INPUT_BYTES],
            binding,
        }
    }

    pub(super) fn upload(&mut self, queue: &wgpu::Queue, capture: &CapturedFrame) {
        let screen = &capture.screen;
        let header = [
            u32::from(screen.start_address),
            u32::from(screen.cursor.address),
            u32::from(screen.cursor.start_scan_line),
            u32::from(screen.cursor.end_scan_line),
            u32::from(capture.blink_on),
            u32::from(capture.blanked),
            0,
            0,
        ];
        for (bytes, value) in self.bytes[..HEADER_BYTES]
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(header)
        {
            bytes.copy_from_slice(&value.to_le_bytes());
        }
        for (bytes, cell) in self.bytes[HEADER_BYTES..]
            .as_chunks_mut::<2>()
            .0
            .iter_mut()
            .zip(&screen.cells)
        {
            bytes.copy_from_slice(&[cell.character, cell.attribute]);
        }
        queue.write_buffer(&self.buffer, 0, &self.bytes);
    }
}

fn texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    size: [u32; 2],
    format: wgpu::TextureFormat,
    bytes: &[u8],
) -> wgpu::TextureView {
    device
        .create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("fixed VGA text resource"),
                size: wgpu::Extent3d {
                    width: size[0],
                    height: size[1],
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            TextureDataOrder::LayerMajor,
            bytes,
        )
        .create_view(&Default::default())
}
