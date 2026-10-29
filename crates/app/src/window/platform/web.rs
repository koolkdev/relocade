use super::host::Event;
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

    pub fn into_surface(self) -> Result<(SurfaceTarget, wgpu::Surface<'static>), String> {
        let target = SurfaceTarget {
            instance: wgpu::Instance::default(),
            canvas: self.0,
        };
        let surface = target.create()?;
        Ok((target, surface))
    }
}

pub(crate) struct SurfaceTarget {
    pub instance: wgpu::Instance,
    canvas: web_sys::OffscreenCanvas,
}
impl SurfaceTarget {
    fn create(&self) -> Result<wgpu::Surface<'static>, String> {
        self.instance
            .create_surface(wgpu::SurfaceTarget::OffscreenCanvas(self.canvas.clone()))
            .map_err(|e| e.to_string())
    }
    pub async fn recreate(
        &self,
        _events: &winit::event_loop::EventLoopProxy<Event>,
    ) -> Result<wgpu::Surface<'static>, String> {
        // Reuse the canvas already owned by this realm; never transfer it again.
        self.create()
    }
    pub fn before_present(&self) {}
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
