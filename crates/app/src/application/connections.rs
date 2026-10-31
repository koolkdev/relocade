use super::{wake::EventWake, Event, Task};
use crate::graphics::{Graphics, SurfaceSetup};
use futures_channel::oneshot;
use relocade_peripherals::{display::DisplayRenderer, keyboard::KeyboardInput};
use std::sync::Arc;
use winit::event_loop::EventLoopProxy;

pub(super) type StartGraphics =
    Box<dyn FnOnce(SurfaceSetup, EventLoopProxy<Event>) -> (Graphics, Task) + Send>;

/// Connections that may be moved to the execution owner. The application and
/// window stay on main; factories still create display implementations on graphics.
pub struct PeripheralConnections {
    pub(super) display: Option<oneshot::Sender<StartGraphics>>,
    pub(super) keyboard: Option<oneshot::Sender<KeyboardInput>>,
    pub(super) wake: Arc<EventWake>,
}

impl PeripheralConnections {
    /// Attaches one display. Its factory runs on graphics with the surface's GPU.
    /// Call once, after the application's source is ready. The display itself
    /// stays on graphics and need not be `Send`.
    pub fn attach_display<D: DisplayRenderer + 'static>(
        &mut self,
        create: impl FnOnce(&wgpu::Device, &wgpu::Queue) -> Result<D, String> + Send + 'static,
    ) -> Result<(), String> {
        self.display
            .take()
            .ok_or("A display is already attached")?
            .send(Box::new(move |setup, events| {
                let (graphics, finished) = Graphics::spawn(setup, create, move || {
                    let _ = events.send_event(Event::RedrawNeeded);
                });
                (graphics, Box::pin(finished))
            }))
            .map_err(|_| "The display window is closing")?;
        self.wake.notify();
        Ok(())
    }

    /// Routes this window's keyboard events to the supplied device input.
    pub fn attach_keyboard(&mut self, input: KeyboardInput) -> Result<(), String> {
        self.keyboard
            .take()
            .ok_or("A keyboard is already attached")?
            .send(input)
            .map_err(|_| "The application is closing")?;
        self.wake.notify();
        Ok(())
    }
}
