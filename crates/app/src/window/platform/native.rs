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
        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| e.to_string())?;
        Ok(Self {
            instance,
            surface,
            window,
        })
    }

    pub fn into_surface(
        self,
    ) -> Result<(wgpu::Instance, wgpu::Surface<'static>, Arc<Window>), String> {
        Ok((self.instance, self.surface, self.window))
    }
}

pub fn detach(_window: &Window) {}
