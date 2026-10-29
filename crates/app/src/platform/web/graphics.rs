use std::sync::Arc;
use wasm_bindgen::{JsCast, JsValue};
use web_thread::{AsJs, Post};
use winit::{event_loop::ActiveEventLoop, platform::web::WindowExtWebSys, window::Window};

/// Transfers only the offscreen canvas; GPU objects are created on its worker.
pub struct SurfaceSetup(web_sys::OffscreenCanvas);

pub(crate) struct SurfaceOwner {
    pub(crate) instance: wgpu::Instance,
    pub(crate) surface: wgpu::Surface<'static>,
}

impl SurfaceSetup {
    pub(crate) fn new(window: Arc<Window>, _: &ActiveEventLoop) -> Result<Self, String> {
        window
            .canvas()
            .expect("surface setup runs on main")
            .transfer_control_to_offscreen()
            .map(Self)
            .map_err(|e| format!("Canvas transfer: {e:?}"))
    }

    pub(crate) fn open(self) -> Result<SurfaceOwner, String> {
        let instance = wgpu::Instance::default();
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::OffscreenCanvas(self.0))
            .map_err(|e| e.to_string())?;
        Ok(SurfaceOwner { instance, surface })
    }
}

impl SurfaceOwner {
    pub(crate) fn present(&self, queue: &wgpu::Queue, frame: wgpu::SurfaceTexture) {
        queue.present(frame);
    }
}

impl AsJs for SurfaceSetup {
    fn to_js(&self) -> Result<JsValue, JsValue> {
        Ok(self.0.clone().into())
    }
    fn from_js(value: JsValue) -> Result<Self, JsValue> {
        value.dyn_into().map(Self)
    }
}

impl Post for SurfaceSetup {
    fn transferables(&self) -> js_sys::Array {
        js_sys::Array::of1(&self.0)
    }
}

/// Gives the browser worker's event loop a turn to dispatch WebGPU events.
pub(crate) async fn yield_to_host() {
    // A timer resumes through a browser task; a self-wake can stay in microtasks.
    futures_timer::Delay::new(std::time::Duration::ZERO).await;
}
