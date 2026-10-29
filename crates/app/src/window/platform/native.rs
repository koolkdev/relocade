use super::host::Event;
use std::sync::Arc;
use winit::{event_loop::ActiveEventLoop, window::Window};

/// Created on the winit thread, then moved exactly once to the graphics worker.
/// Keeping the main-thread Arc alive makes final window destruction a main action.
pub(crate) struct SurfaceSetup {
    pub instance: wgpu::Instance,
    pub surface: wgpu::Surface<'static>,
    pub window: Arc<Window>,
}

impl SurfaceSetup {
    pub fn new(window: Arc<Window>, event_loop: &ActiveEventLoop) -> Result<Self, String> {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle_from_env(
                Box::new(event_loop.owned_display_handle()),
            ));
        // Metal window-surface creation requires main. This is intentionally not
        // part of the graphics worker's asynchronous device initialization.
        let surface = create_surface(&instance, window.clone())?;
        Ok(Self {
            instance,
            surface,
            window,
        })
    }

    pub fn into_surface(self) -> Result<(SurfaceTarget, wgpu::Surface<'static>), String> {
        Ok((
            SurfaceTarget {
                instance: self.instance,
                window: self.window,
            },
            self.surface,
        ))
    }
}

// Initial setup and recovery both enter here on winit's thread (required by Metal).
pub(crate) fn create_surface(
    instance: &wgpu::Instance,
    window: Arc<Window>,
) -> Result<wgpu::Surface<'static>, String> {
    instance
        .create_surface(window)
        .map_err(|error| error.to_string())
}

pub(crate) struct SurfaceTarget {
    pub instance: wgpu::Instance,
    window: Arc<Window>,
}
impl SurfaceTarget {
    pub async fn recreate(
        &self,
        events: &winit::event_loop::EventLoopProxy<Event>,
    ) -> Result<wgpu::Surface<'static>, String> {
        let (reply, surface) = futures_channel::oneshot::channel();
        // Recovery has the same main-thread Metal requirement as initial creation.
        events
            .send_event(Event::RecreateSurface {
                instance: self.instance.clone(),
                window: self.window.clone(),
                reply,
            })
            .map_err(|_| "Window closed during surface recovery")?;
        surface
            .await
            .map_err(|_| "Surface recovery was cancelled")?
    }
    pub fn before_present(&self) {
        self.window.pre_present_notify();
    }
}

pub fn detach(_window: &Window) {}
