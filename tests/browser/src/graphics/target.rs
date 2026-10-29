use futures_channel::oneshot;
use relocade_renderer::{Image, Viewer};
use relocade_vga::VideoFrame;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[cfg(not(target_arch = "wasm32"))]
pub type Context = ();

#[cfg(target_arch = "wasm32")]
pub struct Context(web_sys::OffscreenCanvas);

#[cfg(target_arch = "wasm32")]
impl relocade_machine::worker::AsJs for Context {
    fn to_js(&self) -> Result<wasm_bindgen::JsValue, wasm_bindgen::JsValue> {
        Ok(self.0.clone().into())
    }
    fn from_js(value: wasm_bindgen::JsValue) -> Result<Self, wasm_bindgen::JsValue> {
        use wasm_bindgen::JsCast;
        Ok(Self(value.dyn_into()?))
    }
}

#[cfg(target_arch = "wasm32")]
impl relocade_machine::worker::Post for Context {
    fn transferables(&self) -> js_sys::Array {
        js_sys::Array::of1(&self.0)
    }
}

pub fn context() -> Result<Context, String> {
    #[cfg(target_arch = "wasm32")]
    return web_sys::OffscreenCanvas::new(1, 1)
        .map(Context)
        .map_err(|error| format!("{error:?}"));
    #[cfg(not(target_arch = "wasm32"))]
    Ok(())
}

pub struct Presented {
    pub sequence: u64,
    pub size: [u32; 2],
    pub count: u32,
    pub pixel: [u8; 4],
}

pub struct Target {
    device: wgpu::Device,
    queue: wgpu::Queue,
    viewer: Viewer,
    image: Option<Image>,
    format: wgpu::TextureFormat,
    #[cfg(target_arch = "wasm32")]
    surface: wgpu::Surface<'static>,
    #[cfg(target_arch = "wasm32")]
    configuration: wgpu::SurfaceConfiguration,
    count: u32,
    dropped: Arc<AtomicBool>,
}

impl Target {
    pub async fn new(_context: Context, dropped: Arc<AtomicBool>) -> Result<Self, String> {
        let instance = wgpu::Instance::default();
        #[cfg(target_arch = "wasm32")]
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::OffscreenCanvas(_context.0))
            .map_err(|error| error.to_string())?;
        let options = wgpu::RequestAdapterOptions {
            #[cfg(target_arch = "wasm32")]
            compatible_surface: Some(&surface),
            ..Default::default()
        };
        let adapter = instance
            .request_adapter(&options)
            .await
            .map_err(|e| e.to_string())?;
        let (device, queue) = adapter
            .request_device(&Default::default())
            .await
            .map_err(|e| e.to_string())?;
        #[cfg(target_arch = "wasm32")]
        let configuration = surface.get_default_config(&adapter, 1, 1).unwrap();
        #[cfg(target_arch = "wasm32")]
        let format = configuration.format;
        #[cfg(not(target_arch = "wasm32"))]
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let viewer =
            Viewer::new(device.clone(), queue.clone(), format).map_err(|e| e.to_string())?;
        Ok(Self {
            device,
            queue,
            viewer,
            image: None,
            format,
            #[cfg(target_arch = "wasm32")]
            surface,
            #[cfg(target_arch = "wasm32")]
            configuration,
            count: 0,
            dropped,
        })
    }

    pub async fn present(
        &mut self,
        frame: &VideoFrame,
        size: [u32; 2],
    ) -> Result<Presented, String> {
        let output = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("worker GPU readback"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        if self.image.as_ref().is_none_or(|image| {
            image.size() != [frame.width, frame.height]
                || image.pixel_aspect_ratio() != frame.pixel_aspect_ratio
        }) {
            self.image = Some(
                Image::new(
                    &self.device,
                    [frame.width, frame.height],
                    frame.pixel_aspect_ratio,
                )
                .map_err(|e| e.to_string())?,
            );
        }
        let image = self.image.as_ref().unwrap();
        image
            .upload(&self.queue, &frame.pixels)
            .map_err(|e| e.to_string())?;
        self.viewer
            .draw(image, &output.create_view(&Default::default()), size)
            .map_err(|e| e.to_string())?;
        let pixel = read_first_pixel(&self.device, &self.queue, &output).await;
        #[cfg(target_arch = "wasm32")]
        {
            self.configuration.width = size[0];
            self.configuration.height = size[1];
            self.surface.configure(&self.device, &self.configuration);
            let surface = match self.surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(texture)
                | wgpu::CurrentSurfaceTexture::Suboptimal(texture) => texture,
                other => return Err(format!("surface acquisition: {other:?}")),
            };
            self.viewer
                .draw(
                    image,
                    &surface.texture.create_view(&Default::default()),
                    size,
                )
                .map_err(|e| e.to_string())?;
            self.queue.present(surface);
        }
        self.count += 1;
        Ok(Presented {
            sequence: frame.sequence,
            size,
            count: self.count,
            pixel,
        })
    }
}

impl Drop for Target {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::Release);
    }
}

async fn read_first_pixel(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
) -> [u8; 4] {
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 256,
        mapped_at_creation: false,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(256),
                rows_per_image: Some(1),
            },
        },
        wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);
    let (send, done) = oneshot::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            send.send(result).unwrap();
        });
    #[cfg(not(target_arch = "wasm32"))]
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    done.await.unwrap().unwrap();
    let mapped = buffer.slice(..).get_mapped_range().unwrap();
    let mut pixel: [u8; 4] = mapped[..4].try_into().unwrap();
    if matches!(
        texture.format(),
        wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
    ) {
        pixel.swap(0, 2);
    }
    drop(mapped);
    buffer.unmap();
    pixel
}
