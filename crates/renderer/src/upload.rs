use crate::FrameError;

pub(crate) struct Upload {
    pub size: [u32; 2],
    pub bind_group: wgpu::BindGroup,
    texture: wgpu::Texture,
}

impl Upload {
    pub fn new(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        size: [u32; 2],
        format: wgpu::TextureFormat,
    ) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("pixel frame upload"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("pixel frame"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        });
        Self {
            size,
            bind_group,
            texture,
        }
    }

    pub fn write(&self, queue: &wgpu::Queue, pixels: &[u8]) {
        queue.write_texture(
            self.texture.as_image_copy(),
            pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(self.size[0] * 4),
                rows_per_image: Some(self.size[1]),
            },
            self.texture.size(),
        );
    }
}

pub(crate) fn validate(size: [u32; 2], len: usize, max_dimension: u32) -> Result<(), FrameError> {
    if size.iter().any(|&n| n == 0 || n > max_dimension) {
        return Err(FrameError::InvalidSize);
    }
    let expected = (size[0] as usize)
        .checked_mul(size[1] as usize)
        .and_then(|n| n.checked_mul(4))
        .ok_or(FrameError::InvalidSize)?;
    if len != expected {
        return Err(FrameError::InvalidLength {
            expected,
            actual: len,
        });
    }
    Ok(())
}
