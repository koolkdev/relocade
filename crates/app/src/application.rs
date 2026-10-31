use crate::{graphics::Graphics, Window};
use futures_channel::oneshot;
use relocade_frontend::keyboard::{KeyboardEvent, ModifiersState};
use std::{
    cell::RefCell,
    future::Future,
    pin::Pin,
    rc::Rc,
    sync::Arc,
    task::{Context, Poll, Waker},
};
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoopProxy},
    window::{WindowAttributes, WindowId},
};

mod host;
mod keyboard;
pub use host::AppHost;
use host::Attachments;

mod wake;
use wake::EventWake;

type Task = Pin<Box<dyn Future<Output = Result<(), String>>>>;
type Finished = Box<dyn FnOnce(Result<(), String>)>;

/// Hosts one display window and drives its application lifecycle on main.
///
/// The lifecycle may await startup, attach a display, then await `host.closing()`
/// and clean up its own resources. Returning also requests closure. The lifecycle
/// need not be `Send`; it runs on main.
/// The window is released only after lifecycle and graphics cleanup finish.
/// Errors are returned after cleanup on native platforms and reported by the
/// browser console when the application finishes there.
///
/// As with [`crate::run`], native execution blocks until exit; browser startup
/// returns immediately; an `Ok` browser return only confirms startup.
pub fn run_app<F: Future<Output = Result<(), String>> + 'static>(
    title: &str,
    lifecycle: impl FnOnce(AppHost) -> F + 'static,
) -> Result<(), String> {
    let attributes = crate::platform::application::attributes(title)?;
    let result = Rc::new(RefCell::new(Ok(())));
    let finished = result.clone();
    crate::run(move |events| {
        Application::new(events, attributes, lifecycle, move |outcome| {
            crate::platform::application::report(&outcome);
            *finished.borrow_mut() = outcome;
        })
    })
    .map_err(|error| error.to_string())?;
    let outcome = result.borrow().clone();
    outcome
}

#[derive(Debug)]
pub(crate) enum Event {
    Wake,
    RedrawNeeded,
    Close,
}

struct Application {
    attributes: Option<WindowAttributes>,
    window: Option<Window>,
    wake: Arc<EventWake>,
    attachments: Rc<RefCell<Attachments>>,
    closing: Option<oneshot::Sender<Result<(), String>>>,
    lifecycle: Option<Task>,
    graphics: Option<Graphics>,
    graphics_finished: Option<Task>,
    finished: Option<Finished>,
    error: Option<String>,
    suspended: bool,
}

impl Application {
    fn new<F: Future<Output = Result<(), String>> + 'static>(
        events: EventLoopProxy<Event>,
        attributes: WindowAttributes,
        lifecycle: impl FnOnce(AppHost) -> F,
        finished: impl FnOnce(Result<(), String>) + 'static,
    ) -> Self {
        let wake = Arc::new(EventWake::new(events));
        let attachments = Rc::new(RefCell::new(Attachments::default()));
        let (closing, close) = oneshot::channel();
        let lifecycle = lifecycle(AppHost {
            attachments: attachments.clone(),
            closing: close,
            wake: wake.clone(),
        });
        Self {
            attributes: Some(attributes),
            window: None,
            wake,
            attachments,
            closing: Some(closing),
            lifecycle: Some(Box::pin(lifecycle)),
            graphics: None,
            graphics_finished: None,
            finished: Some(Box::new(finished)),
            error: None,
            suspended: true,
        }
    }

    fn close(&mut self, result: Result<(), String>) {
        if self.error.is_none() {
            self.error = result.err();
        }
        if let Some(closing) = self.closing.take() {
            let _ = closing.send(self.error.clone().map_or(Ok(()), Err));
            let mut attachments = self.attachments.borrow_mut();
            attachments.closed = true;
            attachments.display = None;
            attachments.keyboard = None;
            if let Some(graphics) = self.graphics.take() {
                graphics.close();
            }
        }
    }

    fn request_redraw(&self) {
        if self.suspended || self.closing.is_none() {
            return;
        }
        if let Some(window) = &self.window {
            if window.state().drawable() {
                window.winit().request_redraw();
            }
        }
    }

    fn poll(&mut self, event_loop: &ActiveEventLoop) {
        if self.attributes.is_some() {
            return; // The first resumed callback creates the window before startup.
        }
        let waker = Waker::from(self.wake.clone());
        let mut cx = Context::from_waker(&waker);
        if let Some(task) = &mut self.lifecycle {
            if let Poll::Ready(result) = task.as_mut().poll(&mut cx) {
                self.lifecycle = None;
                self.close(result);
            }
        }
        let display = self.attachments.borrow_mut().display.take();
        if let Some(start) = display {
            let setup = self
                .window
                .as_ref()
                .expect("application starts after window creation")
                .surface_setup(event_loop);
            match setup {
                Ok(setup) => {
                    let (graphics, finished) = start(setup, self.wake.events.clone());
                    self.graphics = Some(graphics);
                    self.graphics_finished = Some(finished);
                }
                Err(error) => self.close(Err(error)),
            }
        }
        if let Some(task) = &mut self.graphics_finished {
            if let Poll::Ready(result) = task.as_mut().poll(&mut cx) {
                self.graphics_finished = None;
                self.graphics = None;
                self.close(result);
            }
        }
        if self.closing.is_none() && self.lifecycle.is_none() && self.graphics_finished.is_none() {
            self.window = None;
            if let Some(finished) = self.finished.take() {
                finished(self.error.take().map_or(Ok(()), Err));
            }
            event_loop.exit();
        }
    }
}

impl ApplicationHandler<Event> for Application {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.suspended = false;
        if let Some(attributes) = self.attributes.take() {
            match Window::new(event_loop, attributes) {
                Ok(window) => self.window = Some(window),
                Err(error) => self.close(Err(error.to_string())),
            }
        }
        self.request_redraw();
        self.poll(event_loop);
    }

    fn suspended(&mut self, _: &ActiveEventLoop) {
        self.suspended = true;
        self.send_key(KeyboardEvent::FocusLost);
        if let Some(graphics) = &mut self.graphics {
            graphics.request_redraw(PhysicalSize::new(0, 0));
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(window) = &mut self.window else {
            return;
        };
        if !window.handle_event(id, &event) {
            return;
        }
        let state = window.state();
        if self.closing.is_some() {
            if let Some(event) = keyboard::translate(&event) {
                self.send_key(event);
            }
        }
        if state.close_requested {
            self.close(Ok(()));
            self.poll(event_loop);
        } else if !self.suspended && self.closing.is_some() {
            match event {
                WindowEvent::RedrawRequested => {
                    if state.drawable() {
                        if let Some(graphics) = &mut self.graphics {
                            graphics.request_redraw(state.size);
                        }
                    }
                }
                WindowEvent::Resized(_)
                | WindowEvent::ScaleFactorChanged { .. }
                | WindowEvent::Occluded(_) => {
                    if !state.drawable() {
                        if let Some(graphics) = &mut self.graphics {
                            graphics.request_redraw(PhysicalSize::new(0, 0));
                        }
                    }
                    self.request_redraw();
                }
                _ => {}
            }
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: Event) {
        match event {
            Event::Wake => {
                self.wake.clear();
                self.poll(event_loop);
            }
            Event::RedrawNeeded => self.request_redraw(),
            Event::Close => {
                self.close(Ok(()));
                self.poll(event_loop);
            }
        }
    }
}

impl Application {
    fn send_key(&mut self, event: KeyboardEvent) {
        let mut attachments = self.attachments.borrow_mut();
        match &event {
            KeyboardEvent::Modifiers(modifiers) => attachments.modifiers = *modifiers,
            KeyboardEvent::FocusLost => attachments.modifiers = ModifiersState::default(),
            KeyboardEvent::Key { .. } => {}
        }
        if let Some(input) = &attachments.keyboard {
            let _ = input.unbounded_send(event);
        }
    }
}

#[cfg(all(test, any(target_os = "linux", target_arch = "wasm32")))]
mod tests;
