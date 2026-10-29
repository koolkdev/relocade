use crate::FrameError;

/// Reusable RGBA8 GPU storage, independent of output size and format.
/// Pixels store sRGB color values and unmodified alpha. Clones share storage;
/// they are not immutable historical images. The caller orders writes and draws.
#[derive(Clone)]
pub struct Image {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    pixel_aspect_ratio: (u32, u32),
}

impl Image {
    /// Format of [`Self::view`]. GPU producers write sRGB-encoded color values.
    pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

    pub fn new(
        device: &wgpu::Device,
        size: [u32; 2],
        pixel_aspect_ratio: (u32, u32),
    ) -> Result<Self, FrameError> {
        if size
            .iter()
            .any(|&n| n == 0 || n > device.limits().max_texture_dimension_2d)
        {
            return Err(FrameError::InvalidSize);
        }
        byte_count(size)?;
        if pixel_aspect_ratio.0 == 0 || pixel_aspect_ratio.1 == 0 {
            return Err(FrameError::InvalidAspectRatio);
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("rendered image"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: Self::FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        Ok(Self {
            texture,
            view,
            pixel_aspect_ratio,
        })
    }

    pub fn size(&self) -> [u32; 2] {
        [self.texture.width(), self.texture.height()]
    }

    /// Pixel width / height, used by the viewer to preserve display proportions.
    pub fn pixel_aspect_ratio(&self) -> (u32, u32) {
        self.pixel_aspect_ratio
    }

    /// Single-sampled attachment for GPU image generation, in [`Self::FORMAT`].
    pub fn view(&self) -> &wgpu::TextureView {
        &self.view
    }

    /// Underlying storage for explicit copies or test readback.
    pub fn texture(&self) -> &wgpu::Texture {
        &self.texture
    }

    /// Queues a tightly packed, top-to-bottom RGBA8 CPU image for upload.
    /// The next queue submission (including a viewer draw) submits this upload.
    /// Call `queue.submit([])` if no other GPU work follows.
    pub fn upload(&self, queue: &wgpu::Queue, pixels: &[u8]) -> Result<(), FrameError> {
        let size = self.size();
        let expected = byte_count(size)?;
        if pixels.len() != expected {
            return Err(FrameError::InvalidLength {
                expected,
                actual: pixels.len(),
            });
        }
        queue.write_texture(
            self.texture.as_image_copy(),
            pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size[0] * 4),
                rows_per_image: Some(size[1]),
            },
            self.texture.size(),
        );
        Ok(())
    }
}

fn byte_count(size: [u32; 2]) -> Result<usize, FrameError> {
    (size[0] as usize)
        .checked_mul(size[1] as usize)
        .and_then(|n| n.checked_mul(4))
        .ok_or(FrameError::InvalidSize)
}
