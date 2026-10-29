use relocade_app::{Application, Context, Input, WindowApplication};
use relocade_renderer::{fit_image, Image, Rect, Renderer};
use std::sync::{
    atomic::{AtomicU8, Ordering},
    Arc,
};

#[derive(Default)]
pub struct TestImage {
    pub dropped: Arc<AtomicU8>,
}
pub struct Source {
    _clock: relocade_machine::TimerHandle,
    dropped: Arc<AtomicU8>,
}
struct ExecutionState(Arc<AtomicU8>);
impl Drop for ExecutionState {
    fn drop(&mut self) {
        self.0.fetch_or(1, Ordering::Release);
    }
}
pub struct Graphics {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: Renderer,
    image: Image,
    dropped: Option<Arc<AtomicU8>>,
}
impl Drop for Graphics {
    fn drop(&mut self) {
        if let Some(dropped) = &self.dropped {
            dropped.fetch_or(2, Ordering::Release);
        }
    }
}
impl Application for TestImage {
    type Source = Source;
    type Input = Input;
    type Output = u8;

    async fn run(self, context: Context<Self::Input, Self::Source>) -> Result<u8, String> {
        let _state = ExecutionState(self.dropped.clone());
        // Retaining a clock in the source must not prevent window shutdown.
        context.publish(Source {
            _clock: context.clock.clone(),
            dropped: self.dropped,
        });
        Ok(7)
    }
}
impl WindowApplication for TestImage {
    type Graphics = Graphics;

    fn create_graphics(
        device: wgpu::Device,
        queue: wgpu::Queue,
        format: wgpu::TextureFormat,
    ) -> Result<Graphics, String> {
        let image = Image::new(&device, [2, 2], (1, 1)).map_err(|e| e.to_string())?;
        image
            .upload(
                &queue,
                &[
                    170, 0, 0, 255, 0, 170, 0, 255, 0, 0, 170, 255, 255, 255, 85, 255,
                ],
            )
            .map_err(|e| e.to_string())?;
        let renderer = Renderer::new(device.clone(), format).map_err(|e| e.to_string())?;
        Ok(Graphics {
            device,
            queue,
            renderer,
            image,
            dropped: None,
        })
    }
    fn draw(
        graphics: &mut Graphics,
        source: &Self::Source,
        attachment: &wgpu::TextureView,
        size: [u32; 2],
    ) -> Result<(), String> {
        graphics.dropped = Some(source.dropped.clone());
        let mut encoder = graphics.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: attachment,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            graphics.renderer.draw_image(
                &mut pass,
                &graphics.image,
                fit_image(&graphics.image, Rect::from_size(size)),
            );
        }
        graphics.queue.submit([encoder.finish()]);
        Ok(())
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn start_image() -> Result<relocade_app::web::BrowserControl, wasm_bindgen::JsValue> {
    console_error_panic_hook::set_once();
    let application = TestImage::default();
    DROPPED.with(|slot| *slot.borrow_mut() = Some(application.dropped.clone()));
    relocade_app::open(application, Default::default())
        .map(relocade_app::web::control)
        .map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}

#[cfg(target_arch = "wasm32")]
thread_local! { static DROPPED: std::cell::RefCell<Option<Arc<AtomicU8>>> = const { std::cell::RefCell::new(None) }; }

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn image_dropped() -> u8 {
    DROPPED.with(|slot| slot.borrow().as_ref().unwrap().load(Ordering::Acquire))
}
