mod scheduler;
mod state;

use crate::{display, platform, session::Session, Application, Control, Renderer, StopHandle};
use futures_channel::oneshot;
use scheduler::FrameScheduler;
use state::{Resume, WindowState};
use std::{
    sync::Arc,
    task::{Context, Wake, Waker},
};
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy},
    window::WindowId,
};

/// Host display lifetime after the application task completes normally.
#[derive(Clone, Copy)]
pub enum OnCompletion {
    Close,
    KeepOpen,
}

/// Properties of the host window or browser canvas, independent of execution.
pub struct WindowOptions {
    pub title: String,
    pub size: [u32; 2],
    pub on_completion: OnCompletion,
}

pub(crate) enum Event<D> {
    Application(D),
    FrameDone,
    #[allow(
        dead_code,
        reason = "browser surfaces are created on the graphics worker"
    )]
    Surface(platform::SurfaceRequest),
    Wake,
    Stop,
}

/// Starts a host window and the application's execution and graphics workers.
/// Native event loops return after shutdown; browser loops return after registration.
/// Await the control for completion. Rendering progresses independently of
/// host redraws, including while the host is hidden or its surface is suspended.
pub fn run<R: Renderer, A: Application<Update = R::Source>>(
    application: A,
    options: WindowOptions,
) -> Result<Control<A::Output>, String> {
    let event_loop = EventLoop::<Event<R::Source>>::with_user_event()
        .build()
        .map_err(|error| error.to_string())?;
    let proxy = event_loop.create_proxy();
    let (finished, completion) = oneshot::channel();
    let host = Host {
        session: Session::<A, R>::new(application, options.on_completion, finished),
        window: WindowState::Pending {
            title: options.title,
            size: options.size,
        },
        frames: FrameScheduler::new(),
        waker: Arc::new(WakeEvents(proxy.clone())).into(),
        proxy: proxy.clone(),
    };
    platform::run(event_loop, host)?;
    let stop = StopHandle(Arc::new(move || {
        let _ = proxy.send_event(Event::Stop);
    }));
    Ok(Control { stop, completion })
}

struct WakeEvents<D: Send + 'static>(EventLoopProxy<Event<D>>);

impl<D: Send + 'static> Wake for WakeEvents<D> {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        let _ = self.0.send_event(Event::Wake);
    }
}

/// Adapts winit events to the session, window lifecycle and frame scheduler.
pub(crate) struct Host<A: Application, R: Renderer> {
    session: Session<A, R>,
    window: WindowState,
    frames: FrameScheduler,
    waker: Waker,
    proxy: EventLoopProxy<Event<R::Source>>,
}

impl<A: Application<Update = R::Source>, R: Renderer> Host<A, R> {
    fn poll_session(&mut self, event_loop: &ActiveEventLoop) {
        self.session.poll(&mut Context::from_waker(&self.waker));
        // Main must still service surface handoffs and worker wakes until both
        // accepted tasks have released their resources, including during shutdown.
        if self.session.is_finished() {
            event_loop.exit();
        }
    }
}

impl<A: Application<Update = R::Source>, R: Renderer> ApplicationHandler<Event<R::Source>>
    for Host<A, R>
{
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.session.is_closing() {
            return;
        }
        let proxy = self.proxy.clone();
        match self.window.resume(event_loop, move |request| {
            proxy
                .send_event(Event::Surface(request))
                .map_err(|_| "The window event loop closed".to_owned())
        }) {
            Ok(Resume::Created(target)) => {
                let updates = self.proxy.clone();
                let frames = self.proxy.clone();
                self.session.start(
                    target,
                    move |update| {
                        let _ = updates.send_event(Event::Application(update));
                    },
                    move || {
                        let _ = frames.send_event(Event::FrameDone);
                    },
                );
            }
            Ok(Resume::Resumed) => {
                self.frames.invalidate();
                self.session.render(display::Command::Resume);
            }
            Ok(Resume::Unchanged) => {}
            Err(error) => self.session.fail(error),
        }
        self.poll_session(event_loop);
    }

    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        if self.window.suspend() {
            self.session.render(display::Command::Suspend);
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: Event<R::Source>) {
        match event {
            Event::Application(source) => self.session.update(source),
            Event::FrameDone => self.frames.complete(),
            Event::Surface(request) => request.complete(),
            Event::Wake => self.poll_session(event_loop),
            Event::Stop => {
                self.session.stop();
                self.poll_session(event_loop);
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if !self.window.matches(id) {
            return;
        }
        match event {
            WindowEvent::CloseRequested | WindowEvent::Destroyed => {
                self.session.stop();
                self.poll_session(event_loop);
            }
            WindowEvent::Resized(size) => {
                self.window.resize(size);
                self.frames.invalidate();
            }
            WindowEvent::Occluded(occluded) => self.window.occlude(occluded),
            WindowEvent::RedrawRequested if !self.session.is_closing() => {
                if let Some(size) = self.window.drawable_size() {
                    if self.frames.submit() {
                        self.session.render(display::Command::Present(size));
                    }
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.poll_session(event_loop);
        event_loop.set_control_flow(if self.session.is_closing() {
            ControlFlow::Wait
        } else {
            self.window.schedule(&self.frames)
        });
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.window.detach();
        self.session.finish();
    }
}
