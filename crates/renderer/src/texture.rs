use crate::Error;
use std::rc::Rc;

/// A worker-local texture, independent of host size, format and visibility.
/// Clones share a reusable texture, not an immutable historical frame.
/// Its RGBA8 bytes encode sRGB colors and unmodified
/// alpha; rendering through `view()` uses linear shader colors and an sRGB attachment.
#[derive(Clone)]
pub struct Texture(Rc<Storage>);

struct Storage {
    texture: wgpu::Texture,
    srgb: wgpu::TextureView,
    unorm: wgpu::TextureView,
}

impl Texture {
    pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

    pub fn new(device: &wgpu::Device, size: [u32; 2]) -> Result<Self, Error> {
        if size
            .iter()
            .any(|&n| n == 0 || n > device.limits().max_texture_dimension_2d)
        {
            return Err(Error::InvalidSize);
        }
        byte_count(size)?;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("rendered texture"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: Self::FORMAT,
            usage: wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[wgpu::TextureFormat::Rgba8Unorm],
        });
        let srgb = texture.create_view(&Default::default());
        let unorm = texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(wgpu::TextureFormat::Rgba8Unorm),
            ..Default::default()
        });
        Ok(Self(Rc::new(Storage {
            texture,
            srgb,
            unorm,
        })))
    }

    pub fn size(&self) -> [u32; 2] {
        [self.0.texture.width(), self.0.texture.height()]
    }

    /// A single-sampled sRGB attachment for direct GPU rendering.
    pub fn view(&self) -> &wgpu::TextureView {
        &self.0.srgb
    }

    /// The texture also supports copies and readback.
    pub fn raw(&self) -> &wgpu::Texture {
        &self.0.texture
    }

    /// Uploads tightly packed, top-to-bottom RGBA8 bytes and submits the upload.
    /// Uploads progress even when the host never draws or presents.
    pub fn write(&self, queue: &wgpu::Queue, pixels: &[u8]) -> Result<(), Error> {
        let size = self.size();
        let expected = byte_count(size)?;
        if pixels.len() != expected {
            return Err(Error::InvalidLength {
                expected,
                actual: pixels.len(),
            });
        }
        queue.write_texture(
            self.0.texture.as_image_copy(),
            pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size[0] * 4),
                rows_per_image: Some(size[1]),
            },
            self.0.texture.size(),
        );
        queue.submit([]);
        Ok(())
    }

    pub(crate) fn sampling_view(&self, srgb_output: bool) -> &wgpu::TextureView {
        // An sRGB attachment encodes linear shader results. For an unorm host
        // attachment, sample the stored sRGB bytes without decoding them.
        if srgb_output {
            &self.0.srgb
        } else {
            &self.0.unorm
        }
    }

    pub(crate) fn same_texture(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

fn byte_count(size: [u32; 2]) -> Result<usize, Error> {
    (size[0] as usize)
        .checked_mul(size[1] as usize)
        .and_then(|n| n.checked_mul(4))
        .ok_or(Error::InvalidSize)
}
