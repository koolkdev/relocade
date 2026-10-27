use crate::{
    window::{Event, Host},
    Application, Renderer,
};
use futures_channel::oneshot;
use std::sync::Arc;
use winit::{
    event_loop::{ActiveEventLoop, EventLoop},
    window::{Window, WindowAttributes},
};

pub(crate) fn run<R: Renderer, A: Application<Update = R::Source>>(
    event_loop: EventLoop<Event<R::Source>>,
    mut app: Host<A, R>,
) -> Result<(), String> {
    event_loop
        .run_app(&mut app)
        .map_err(|error| error.to_string())
}

pub(crate) fn window_attributes(attributes: WindowAttributes) -> WindowAttributes {
    attributes
}

pub(crate) fn detach_window(_window: &Window) {}

pub(crate) struct RenderTarget {
    window: Arc<Window>,
    instance: wgpu::Instance,
    request_surface: Box<dyn Fn(SurfaceRequest) -> Result<(), String> + Send + Sync>,
}

impl RenderTarget {
    pub fn new(
        window: Arc<Window>,
        event_loop: &ActiveEventLoop,
        request_surface: impl Fn(SurfaceRequest) -> Result<(), String> + Send + Sync + 'static,
    ) -> Result<Self, String> {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle_from_env(
                Box::new(event_loop.owned_display_handle()),
            ));
        Ok(Self {
            window,
            instance,
            request_surface: Box::new(request_surface),
        })
    }

    pub fn instance(&self) -> wgpu::Instance {
        self.instance.clone()
    }

    pub async fn surface(
        &self,
        instance: &wgpu::Instance,
    ) -> Result<wgpu::Surface<'static>, String> {
        let (reply, surface) = oneshot::channel();
        (self.request_surface)(SurfaceRequest {
            window: self.window.clone(),
            instance: instance.clone(),
            reply,
        })?;
        surface
            .await
            .map_err(|_| "Surface creation was cancelled".to_owned())?
    }

    pub fn pre_present_notify(&self) {
        self.window.pre_present_notify();
    }
}

/// Surface creation runs on the event-loop thread on every native platform.
pub(crate) struct SurfaceRequest {
    window: Arc<Window>,
    instance: wgpu::Instance,
    reply: oneshot::Sender<Result<wgpu::Surface<'static>, String>>,
}

impl SurfaceRequest {
    pub fn complete(self) {
        if !self.reply.is_canceled() {
            let result = self
                .instance
                .create_surface(self.window)
                .map_err(|error| error.to_string());
            let _ = self.reply.send(result);
        }
    }
}
