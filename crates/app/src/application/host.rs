use super::{wake::EventWake, Event, Task};
use crate::graphics::{Graphics, SurfaceSetup};
use futures_channel::oneshot;
use relocade_frontend::{
    display::DisplayRenderer,
    keyboard::{KeyboardEvent, KeyboardInput, ModifiersState},
};
use std::{cell::RefCell, rc::Rc, sync::Arc};
use winit::event_loop::EventLoopProxy;

pub(super) type StartGraphics =
    Box<dyn FnOnce(SurfaceSetup, EventLoopProxy<Event>) -> (Graphics, Task) + Send>;

#[derive(Default)]
pub(super) struct Attachments {
    pub(super) display: Option<StartGraphics>,
    display_attached: bool,
    pub(super) keyboard: Option<KeyboardInput>,
    pub(super) modifiers: ModifiersState,
    pub(super) closed: bool,
}

/// Main-thread access to a window's input, display and application lifetime.
/// Execution supplies ordinary endpoints; attachment stays on the application
/// thread. Display factories construct their adapters on graphics.
pub struct AppHost {
    pub(super) attachments: Rc<RefCell<Attachments>>,
    pub(super) closing: oneshot::Receiver<Result<(), String>>,
    pub(super) wake: Arc<EventWake>,
}

impl AppHost {
    /// Attaches one display to this window. The factory runs on graphics with
    /// the surface's GPU; the display itself need not be `Send`.
    pub fn attach_display<D: DisplayRenderer + 'static>(
        &mut self,
        create: impl FnOnce(&wgpu::Device, &wgpu::Queue) -> Result<D, String> + Send + 'static,
    ) -> Result<(), String> {
        let mut attachments = self.attachments.borrow_mut();
        if attachments.closed {
            return Err("The display window is closing".into());
        }
        if attachments.display_attached {
            return Err("A display is already attached".into());
        }
        attachments.display_attached = true;
        attachments.display = Some(Box::new(move |setup, events| {
            let (graphics, finished) = Graphics::spawn(setup, create, move || {
                let _ = events.send_event(Event::RedrawNeeded);
            });
            (graphics, Box::pin(finished))
        }));
        self.wake.notify();
        Ok(())
    }

    /// Routes this window's keyboard events to an execution-side input adapter.
    pub fn attach_keyboard(&mut self, input: KeyboardInput) -> Result<(), String> {
        let mut attachments = self.attachments.borrow_mut();
        if attachments.closed {
            return Err("The application is closing".into());
        }
        if attachments.keyboard.is_some() {
            return Err("A keyboard is already attached".into());
        }
        let _ = input.unbounded_send(KeyboardEvent::Modifiers(attachments.modifiers));
        attachments.keyboard = Some(input);
        Ok(())
    }

    /// Requests the same orderly shutdown as closing the window.
    pub fn request_close(&self) {
        let _ = self.wake.events.send_event(Event::Close);
    }

    /// Waits for a window close, startup failure or graphics error.
    /// Keep driving host services while shutting down application-owned workers,
    /// then return from the lifecycle future. This wait may be cancelled and
    /// resumed until it completes; await its result only once.
    pub async fn closing(&mut self) -> Result<(), String> {
        (&mut self.closing)
            .await
            .unwrap_or_else(|_| Err("Application host was released".into()))
    }
}
