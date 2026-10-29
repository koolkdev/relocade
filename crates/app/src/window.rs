use crate::platform;
use std::{marker::PhantomData, rc::Rc};
use winit::{
    dpi::PhysicalSize,
    error::OsError,
    event::WindowEvent,
    event_loop::ActiveEventLoop,
    window::{Window as WinitWindow, WindowAttributes, WindowId},
};

/// Current output state, updated from this window's winit events.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowState {
    pub size: PhysicalSize<u32>,
    pub scale_factor: f64,
    /// Winit reports occlusion where supported, including browser page visibility.
    pub occluded: bool,
    /// The application decides when to finish shutdown and release the window.
    pub close_requested: bool,
}

impl WindowState {
    pub fn drawable(self) -> bool {
        !self.occluded && !self.close_requested && self.size.width > 0 && self.size.height > 0
    }
}

/// Main-thread owner of a native window or browser canvas.
///
/// Create in `ApplicationHandler::resumed`, pass window events to `handle_event`,
/// and release after the application's users of the window have shut down.
/// On web, creation attaches the canvas if necessary and dropping removes it.
/// Browser dimensions follow CSS unless explicitly set in the window attributes.
pub struct Window {
    window: WinitWindow,
    state: WindowState,
    _main_thread: PhantomData<Rc<()>>,
}

impl Window {
    pub fn new(
        event_loop: &ActiveEventLoop,
        attributes: WindowAttributes,
    ) -> Result<Self, OsError> {
        let window = platform::create_window(event_loop, attributes)?;
        let state = WindowState {
            size: window.inner_size(),
            scale_factor: window.scale_factor(),
            occluded: false,
            close_requested: false,
        };
        Ok(Self {
            window,
            state,
            _main_thread: PhantomData,
        })
    }

    /// Existing winit operations, including input configuration and redraw requests.
    pub fn winit(&self) -> &WinitWindow {
        &self.window
    }

    pub fn state(&self) -> WindowState {
        self.state
    }

    /// Updates lifecycle state and returns whether this event belongs to this window.
    /// Input and redraw handling remain with the application's `ApplicationHandler`.
    pub fn handle_event(&mut self, id: WindowId, event: &WindowEvent) -> bool {
        if id != self.window.id() {
            return false;
        }
        match event {
            WindowEvent::Resized(size) => self.state.size = *size,
            // Winit follows a scale change with Resized if the physical size changes.
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.state.scale_factor = *scale_factor;
            }
            WindowEvent::Occluded(occluded) => self.state.occluded = *occluded,
            WindowEvent::CloseRequested | WindowEvent::Destroyed => {
                self.state.close_requested = true
            }
            _ => {}
        }
        true
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        platform::detach_window(&self.window);
    }
}
