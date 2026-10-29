use relocade_app::{Application, Context, Input, WindowApplication};
use relocade_renderer::{fit_image, Image, Rect, Renderer};
use std::sync::{
    atomic::{AtomicU32, AtomicU8, Ordering},
    Arc,
};
pub mod stress;

#[derive(Default)]
pub struct TestImage {
    pub observations: Arc<Observations>,
}
#[derive(Default)]
pub struct Observations {
    pub dropped: AtomicU8,
    pub draws: AtomicU32,
    pub worker_tasks: AtomicU32,
}
pub struct Source {
    _clock: relocade_machine::TimerHandle,
    observations: Arc<Observations>,
}
struct ExecutionState(Arc<Observations>);
impl Drop for ExecutionState {
    fn drop(&mut self) {
        self.0.dropped.fetch_or(1, Ordering::Release);
    }
}
pub struct Graphics {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: Renderer,
    image: Image,
    observations: Option<Arc<Observations>>,
    #[cfg(target_arch = "wasm32")]
    heartbeat: Option<heartbeat::Heartbeat>,
}
impl Drop for Graphics {
    fn drop(&mut self) {
        if let Some(observations) = &self.observations {
            observations.dropped.fetch_or(2, Ordering::Release);
        }
    }
}
impl Application for TestImage {
    type Source = Source;
    type Input = Input;
    type Output = u8;

    async fn run(self, context: Context<Self::Input, Self::Source>) -> Result<u8, String> {
        let _state = ExecutionState(self.observations.clone());
        // Retaining a clock in the source must not prevent window shutdown.
        context.publish(Source {
            _clock: context.clock.clone(),
            observations: self.observations,
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
            observations: None,
            #[cfg(target_arch = "wasm32")]
            heartbeat: None,
        })
    }
    fn draw(
        graphics: &mut Graphics,
        source: &Self::Source,
        attachment: &wgpu::TextureView,
        size: [u32; 2],
    ) -> Result<(), String> {
        graphics.observations = Some(source.observations.clone());
        #[cfg(target_arch = "wasm32")]
        graphics
            .heartbeat
            .get_or_insert_with(|| heartbeat::Heartbeat::new(source.observations.clone()));
        source.observations.draws.fetch_add(1, Ordering::Release);
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
    OBSERVATIONS.with(|slot| *slot.borrow_mut() = Some(application.observations.clone()));
    relocade_app::open(application, Default::default())
        .map(|control| relocade_app::web::control(control, Into::into))
        .map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}

#[cfg(target_arch = "wasm32")]
thread_local! { static OBSERVATIONS: std::cell::RefCell<Option<Arc<Observations>>> = const { std::cell::RefCell::new(None) }; }

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn image_dropped() -> u8 {
    OBSERVATIONS.with(|slot| {
        slot.borrow()
            .as_ref()
            .unwrap()
            .dropped
            .load(Ordering::Acquire)
    })
}

#[cfg(target_arch = "wasm32")]
mod heartbeat;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn image_draws() -> u32 {
    OBSERVATIONS.with(|slot| {
        slot.borrow()
            .as_ref()
            .unwrap()
            .draws
            .load(Ordering::Acquire)
    })
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn image_worker_tasks() -> u32 {
    OBSERVATIONS.with(|slot| {
        slot.borrow()
            .as_ref()
            .unwrap()
            .worker_tasks
            .load(Ordering::Acquire)
    })
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn start_dos(
    program: &[u8],
) -> Result<relocade_app::web::BrowserControl, wasm_bindgen::JsValue> {
    relocade_dos_app::open(program)
}
