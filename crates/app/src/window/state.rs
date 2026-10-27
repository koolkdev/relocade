use super::scheduler::FrameScheduler;
use crate::platform::{self, RenderTarget, SurfaceRequest};
use std::{mem, sync::Arc};
use winit::{
    dpi::{LogicalSize, PhysicalSize},
    event_loop::{ActiveEventLoop, ControlFlow},
    window::{Window, WindowId},
};

pub(super) enum Resume {
    Created(RenderTarget),
    Resumed,
    Unchanged,
}

pub(super) enum WindowState {
    Pending { title: String, size: [u32; 2] },
    Live(LiveWindow),
    Closed,
}

pub(super) struct LiveWindow {
    window: Arc<Window>,
    size: PhysicalSize<u32>,
    resumed: bool,
    occluded: bool,
}

impl WindowState {
    pub fn resume(
        &mut self,
        event_loop: &ActiveEventLoop,
        request_surface: impl Fn(SurfaceRequest) -> Result<(), String> + Send + Sync + 'static,
    ) -> Result<Resume, String> {
        match self {
            Self::Pending { title, size } => {
                let attributes = platform::window_attributes(
                    Window::default_attributes()
                        .with_title(title.as_str())
                        .with_inner_size(LogicalSize::new(size[0], size[1])),
                );
                let window = Arc::new(
                    event_loop
                        .create_window(attributes)
                        .map_err(|error| error.to_string())?,
                );
                *self = Self::Live(LiveWindow {
                    size: window.inner_size(),
                    window: window.clone(),
                    resumed: true,
                    occluded: false,
                });
                RenderTarget::new(window, event_loop, request_surface).map(Resume::Created)
            }
            Self::Live(live) if !live.resumed => {
                live.resumed = true;
                Ok(Resume::Resumed)
            }
            _ => Ok(Resume::Unchanged),
        }
    }

    pub fn suspend(&mut self) -> bool {
        match self {
            Self::Live(live) => mem::replace(&mut live.resumed, false),
            _ => false,
        }
    }

    pub fn matches(&self, id: WindowId) -> bool {
        matches!(self, Self::Live(live) if live.window.id() == id)
    }

    pub fn resize(&mut self, size: PhysicalSize<u32>) {
        if let Self::Live(live) = self {
            live.size = size;
        }
    }

    pub fn occlude(&mut self, occluded: bool) {
        if let Self::Live(live) = self {
            live.occluded = occluded;
        }
    }

    pub fn drawable_size(&self) -> Option<[u32; 2]> {
        match self {
            Self::Live(live)
                if live.resumed
                    && !live.occluded
                    && live.size.width != 0
                    && live.size.height != 0 =>
            {
                Some([live.size.width, live.size.height])
            }
            _ => None,
        }
    }

    pub fn schedule(&self, frames: &FrameScheduler) -> ControlFlow {
        match self {
            Self::Live(live) if self.drawable_size().is_some() => frames.schedule(&live.window),
            _ => ControlFlow::Wait,
        }
    }

    pub fn detach(&mut self) {
        *self = Self::Closed;
    }
}

impl Drop for LiveWindow {
    fn drop(&mut self) {
        platform::detach_window(&self.window);
    }
}
