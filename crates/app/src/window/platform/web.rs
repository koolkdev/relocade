use relocade_machine::worker::{AsJs, Post};
use std::sync::Arc;
use wasm_bindgen::{JsCast, JsValue};
use winit::{event_loop::ActiveEventLoop, platform::web::WindowExtWebSys, window::Window};

/// Only the canvas crosses JavaScript realms. GPU objects are created on its worker.
pub(crate) struct SurfaceSetup(web_sys::OffscreenCanvas);

impl SurfaceSetup {
    pub fn new(window: Arc<Window>, _event_loop: &ActiveEventLoop) -> Result<Self, String> {
        let canvas = window.canvas().ok_or("Missing window canvas")?;
        canvas.set_id("relocade-screen");
        canvas
            .transfer_control_to_offscreen()
            .map(Self)
            .map_err(|e| format!("Canvas transfer: {e:?}"))
    }

    pub fn into_surface(self) -> Result<(wgpu::Instance, wgpu::Surface<'static>, ()), String> {
        let instance = wgpu::Instance::default();
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::OffscreenCanvas(self.0))
            .map_err(|e| e.to_string())?;
        Ok((instance, surface, ()))
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

pub fn detach(window: &Window) {
    if let Some(canvas) = window.canvas() {
        canvas.remove();
    }
}
