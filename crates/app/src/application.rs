use crate::{graphics::Graphics, Window};
use futures_channel::oneshot;
use std::{
    cell::RefCell,
    future::Future,
    pin::Pin,
    rc::Rc,
    sync::Arc,
    task::{Context, Poll, Waker},
};
use winit::{
    event_loop::{ActiveEventLoop, EventLoopProxy},
    window::WindowAttributes,
};

mod events;
mod host;
mod keyboard;
pub use host::AppHost;
use host::HostState;

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

#[derive(Default, PartialEq, Eq)]
enum Phase {
    #[default]
    Running,
    Closing,
    Finished,
}

struct Application {
    attributes: Option<WindowAttributes>,
    window: Option<Window>,
    wake: Arc<EventWake>,
    host: Rc<RefCell<HostState>>,
    closing: Option<oneshot::Sender<Result<(), String>>>,
    lifecycle: Option<Task>,
    graphics: Option<Graphics>,
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
        let host = Rc::new(RefCell::new(HostState::default()));
        let (closing, close) = oneshot::channel();
        let lifecycle = lifecycle(AppHost {
            state: host.clone(),
            closing: close,
            wake: wake.clone(),
        });
        Self {
            attributes: Some(attributes),
            window: None,
            wake,
            host,
            closing: Some(closing),
            lifecycle: Some(Box::pin(lifecycle)),
            graphics: None,
            finished: Some(Box::new(finished)),
            error: None,
            suspended: true,
        }
    }

    fn close(&mut self, result: Result<(), String>) {
        let mut host = self.host.borrow_mut();
        if host.phase == Phase::Finished {
            return;
        }
        if self.error.is_none() {
            self.error = result.err();
        }
        if host.phase != Phase::Running {
            return;
        }
        host.phase = Phase::Closing;
        host.display = None;
        host.keyboard = None;
        if let Some(closing) = self.closing.take() {
            let _ = closing.send(self.error.clone().map_or(Ok(()), Err));
        }
        if let Some(graphics) = &mut self.graphics {
            graphics.request_close();
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
        let display = self.host.borrow_mut().display.take();
        if let Some(start) = display {
            let setup = self
                .window
                .as_ref()
                .expect("application starts after window creation")
                .surface_setup(event_loop);
            match setup {
                Ok(setup) => {
                    self.graphics = Some(start(setup, self.wake.events.clone()));
                }
                Err(error) => self.close(Err(error)),
            }
        }
        if let Some(graphics) = &mut self.graphics {
            let result = std::pin::pin!(graphics.finished()).as_mut().poll(&mut cx);
            if let Poll::Ready(result) = result {
                self.graphics = None;
                self.close(result);
            }
        }
        self.finish(event_loop);
    }

    fn finish(&mut self, event_loop: &ActiveEventLoop) {
        if self.host.borrow().phase != Phase::Closing
            || self.lifecycle.is_some()
            || self.graphics.is_some()
        {
            return;
        }
        self.host.borrow_mut().phase = Phase::Finished;
        self.window = None;
        if let Some(finished) = self.finished.take() {
            finished(self.error.take().map_or(Ok(()), Err));
        }
        event_loop.exit();
    }
}

#[cfg(all(test, any(target_os = "linux", target_arch = "wasm32")))]
mod tests;
