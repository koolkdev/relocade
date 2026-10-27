use crate::{
    window::{Event, Host},
    Application, Renderer,
};
use std::sync::Arc;
use wasm_bindgen::{JsCast, JsValue};
use web_thread::{AsJs, Post};
use winit::{
    event_loop::{ActiveEventLoop, EventLoop},
    platform::web::{EventLoopExtWebSys, WindowAttributesExtWebSys, WindowExtWebSys},
    window::{Window, WindowAttributes},
};

pub(crate) fn run<R: Renderer, A: Application<Update = R::Source>>(
    event_loop: EventLoop<Event<R::Source>>,
    app: Host<A, R>,
) -> Result<(), String> {
    event_loop.spawn_app(app);
    Ok(())
}

pub(crate) fn window_attributes(attributes: WindowAttributes) -> WindowAttributes {
    attributes.with_append(true)
}

pub(crate) fn detach_window(window: &Window) {
    if let Some(canvas) = window.canvas() {
        canvas.remove();
    }
}

/// The canvas crosses the JavaScript boundary once and then stays on its worker.
pub(crate) struct RenderTarget(web_sys::OffscreenCanvas);

impl RenderTarget {
    pub fn new(
        window: Arc<Window>,
        _event_loop: &ActiveEventLoop,
        _request_surface: impl Fn(SurfaceRequest) -> Result<(), String> + Send + Sync + 'static,
    ) -> Result<Self, String> {
        let canvas = window.canvas().ok_or("The window has no browser canvas")?;
        canvas.set_id("relocade-screen");
        canvas
            .transfer_control_to_offscreen()
            .map(Self)
            .map_err(|error| format!("Cannot transfer the canvas to its renderer: {error:?}"))
    }

    pub fn instance(&self) -> wgpu::Instance {
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle())
    }

    pub async fn surface(
        &self,
        instance: &wgpu::Instance,
    ) -> Result<wgpu::Surface<'static>, String> {
        instance
            .create_surface(wgpu::SurfaceTarget::OffscreenCanvas(self.0.clone()))
            .map_err(|error| error.to_string())
    }

    pub fn pre_present_notify(&self) {}
}

impl AsJs for RenderTarget {
    fn to_js(&self) -> Result<JsValue, JsValue> {
        Ok(self.0.clone().into())
    }

    fn from_js(value: JsValue) -> Result<Self, JsValue> {
        value.dyn_into().map(Self)
    }
}

impl Post for RenderTarget {
    fn transferables(&self) -> js_sys::Array {
        let transfer = js_sys::Array::new();
        transfer.push(&self.0);
        transfer
    }
}

/// Browser surfaces are created directly from the worker's retained canvas.
pub(crate) enum SurfaceRequest {}

impl SurfaceRequest {
    pub fn complete(self) {
        match self {}
    }
}
