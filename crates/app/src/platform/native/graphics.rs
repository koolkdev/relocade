use std::sync::Arc;
use winit::{event_loop::ActiveEventLoop, window::Window};

/// Native surface creation happens on main, including macOS/Metal setup.
pub struct SurfaceSetup {
    pub(crate) instance: wgpu::Instance,
    pub(crate) surface: wgpu::Surface<'static>,
    window: Arc<Window>,
}

pub(crate) type SurfaceOwner = SurfaceSetup;

impl SurfaceSetup {
    pub(crate) fn new(window: Arc<Window>, event_loop: &ActiveEventLoop) -> Result<Self, String> {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle_from_env(
                Box::new(event_loop.owned_display_handle()),
            ));
        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| e.to_string())?;
        Ok(Self {
            instance,
            surface,
            window,
        })
    }

    pub(crate) fn open(self) -> Result<SurfaceOwner, String> {
        Ok(self)
    }

    pub(crate) fn present(&self, queue: &wgpu::Queue, frame: wgpu::SurfaceTexture) {
        self.window.pre_present_notify();
        queue.present(frame);
    }
}

/// Returns control to the Rust executor between batches of ready graphics work.
pub(crate) async fn yield_to_host() {
    futures_lite::future::yield_now().await;
}
