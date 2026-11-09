use super::{wake::EventWake, Event, Phase};
use crate::graphics::{Graphics, SurfaceSetup};
use relocade_frontend::{
    display::DisplayRenderer,
    keyboard::{KeyboardEvent, KeyboardInput, ModifiersState},
    mouse::{MouseEvent, MouseInput},
};
use std::{cell::RefCell, future::Future, rc::Rc, sync::Arc};
use winit::event_loop::EventLoopProxy;

pub(super) type StartGraphics =
    Box<dyn FnOnce(SurfaceSetup, EventLoopProxy<Event>) -> Graphics + Send>;

#[derive(Default)]
pub(super) struct HostState {
    #[cfg(feature = "audio")]
    pub(super) audio: Option<crate::audio::Audio>,
    pub(super) display: Option<StartGraphics>,
    display_attached: bool,
    pub(super) keyboard: Option<KeyboardInput>,
    pub(super) mouse: Option<MouseInput>,
    modifiers: ModifiersState,
    pub(super) phase: Phase,
}

impl HostState {
    pub(super) fn release_input(&mut self) {
        self.send_key(KeyboardEvent::FocusLost);
        self.send_mouse(MouseEvent::FocusLost);
    }

    pub(super) fn send_mouse(&mut self, event: MouseEvent) {
        if let Some(input) = &self.mouse {
            if input.unbounded_send(event).is_err() {
                self.mouse = None;
            }
        }
    }

    pub(super) fn send_key(&mut self, event: KeyboardEvent) {
        match &event {
            KeyboardEvent::Modifiers(modifiers) => self.modifiers = *modifiers,
            KeyboardEvent::FocusLost => self.modifiers = ModifiersState::default(),
            KeyboardEvent::Key { .. } => {}
        }
        if let Some(input) = &self.keyboard {
            let _ = input.unbounded_send(event);
        }
    }
}

/// Main-thread access to a window's input, display and application lifetime.
/// Execution supplies ordinary endpoints; attachment stays on the application
/// thread. Display factories construct their adapters on graphics.
pub struct AppHost {
    pub(super) state: Rc<RefCell<HostState>>,
    pub(super) closing: async_channel::Receiver<Result<(), String>>,
    pub(super) wake: Arc<EventWake>,
}

impl AppHost {
    /// Opens default output and retains it until the window starts closing.
    /// Native playback starts immediately; browsers wait for user activation.
    /// Pass the returned CPAL configuration and bounded sender to the producer.
    #[cfg(feature = "audio")]
    pub fn open_audio(
        &mut self,
    ) -> Result<(cpal::StreamConfig, async_channel::Sender<Vec<f32>>), String> {
        let mut state = self.state.borrow_mut();
        if state.phase != Phase::Running {
            return Err("The application is closing".into());
        }
        if state.audio.is_some() {
            return Err("Audio output is already attached".into());
        }
        let (audio, config, samples) =
            crate::audio::Audio::open(crate::platform::application::report_audio_error)
                .map_err(|error| error.to_string())?;
        crate::platform::application::start_audio(&audio).map_err(|error| error.to_string())?;
        state.audio = Some(audio);
        Ok((config, samples))
    }

    /// Attaches one display to this window. The factory runs on graphics with
    /// the surface's GPU; the display itself need not be `Send`.
    pub fn attach_display<D: DisplayRenderer + 'static>(
        &mut self,
        create: impl FnOnce(&wgpu::Device, &wgpu::Queue) -> Result<D, String> + Send + 'static,
    ) -> Result<(), String> {
        let mut state = self.state.borrow_mut();
        if state.phase != Phase::Running {
            return Err("The display window is closing".into());
        }
        if state.display_attached {
            return Err("A display is already attached".into());
        }
        state.display_attached = true;
        state.display = Some(Box::new(move |setup, events| {
            Graphics::spawn(setup, create, move || {
                let _ = events.send_event(Event::RedrawNeeded);
            })
        }));
        self.wake.notify();
        Ok(())
    }

    /// Routes this window's keyboard events to an execution-side input adapter.
    pub fn attach_keyboard(&mut self, input: KeyboardInput) -> Result<(), String> {
        let mut state = self.state.borrow_mut();
        if state.phase != Phase::Running {
            return Err("The application is closing".into());
        }
        if state.keyboard.is_some() {
            return Err("A keyboard is already attached".into());
        }
        let _ = input.unbounded_send(KeyboardEvent::Modifiers(state.modifiers));
        state.keyboard = Some(input);
        Ok(())
    }

    /// Routes relative motion and buttons to an execution-side input adapter.
    /// Host units are preserved; guest sensitivity and scaling belong to the adapter.
    pub fn attach_mouse(&mut self, input: MouseInput) -> Result<(), String> {
        let mut state = self.state.borrow_mut();
        if state.phase != Phase::Running {
            return Err("The application is closing".into());
        }
        if state.mouse.is_some() {
            return Err("A mouse is already attached".into());
        }
        state.mouse = Some(input);
        Ok(())
    }

    /// Requests the same orderly shutdown as closing the window.
    pub fn request_close(&self) {
        let _ = self.wake.events.send_event(Event::Close);
    }

    /// Waits for a window close, startup failure or graphics error.
    /// Keep driving host services while shutting down application-owned workers,
    /// then return from the lifecycle future. This wait may be cancelled and
    /// resumed until it completes; await its result only once. The returned
    /// future does not borrow the host, so attachment can continue while waiting.
    pub fn closing(&self) -> impl Future<Output = Result<(), String>> + 'static {
        let closing = self.closing.clone();
        async move {
            closing
                .recv()
                .await
                .unwrap_or_else(|_| Err("Application host was released".into()))
        }
    }
}
