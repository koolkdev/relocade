use super::{platform, session::Session, Input, WindowApplication, WindowOptions};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    task::{Context, Wake, Waker},
};
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy},
    window::{Window, WindowId},
};

pub(crate) enum Event {
    Wake,
    GraphicsReady,
    Presented,
    Stop,
}

pub(crate) struct Host<A: WindowApplication> {
    session: Session<A>,
    options: WindowOptions,
    window: Option<Arc<Window>>,
    proxy: EventLoopProxy<Event>,
    wake: Arc<WakeEvents>,
    ready: bool,
    size: [u32; 2],
    suspended: bool,
    occluded: bool,
}

impl<A: WindowApplication> Host<A> {
    pub fn new(session: Session<A>, options: WindowOptions, proxy: EventLoopProxy<Event>) -> Self {
        let wake = Arc::new(WakeEvents {
            proxy: proxy.clone(),
            queued: AtomicBool::new(false),
        });
        Self {
            session,
            options,
            window: None,
            proxy,
            wake,
            ready: false,
            size: [0, 0],
            suspended: false,
            occluded: false,
        }
    }
    fn poll(&mut self, event_loop: &ActiveEventLoop) {
        let waker: Waker = self.wake.clone().into();
        if self.session.poll(&mut Context::from_waker(&waker)) {
            event_loop.exit();
        }
    }
    fn visible(&self) -> bool {
        !self.suspended && !self.occluded
    }
    fn update_output(&self) {
        self.session.output(self.size, self.visible());
    }
    fn request_redraw(&self) {
        if self.ready && self.visible() && !self.size.contains(&0) && !self.session.closing() {
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }
    }
}
impl<A: WindowApplication> ApplicationHandler<Event> for Host<A> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.session.closing() {
            return;
        }
        self.suspended = false;
        if self.window.is_none() {
            let attributes = Window::default_attributes().with_title(&self.options.title);
            #[cfg(not(target_arch = "wasm32"))]
            let attributes = attributes.with_inner_size(winit::dpi::LogicalSize::new(
                self.options.size[0],
                self.options.size[1],
            ));
            #[cfg(target_arch = "wasm32")]
            let attributes = {
                use winit::platform::web::WindowAttributesExtWebSys;
                // CSS owns browser layout; do not install fixed inline dimensions.
                attributes.with_append(true)
            };
            let result = event_loop
                .create_window(attributes)
                .map_err(|e| e.to_string())
                .and_then(|window| {
                    let window = Arc::new(window);
                    let setup = platform::SurfaceSetup::new(window.clone(), event_loop)?;
                    self.size = window.inner_size().into();
                    self.window = Some(window);
                    self.session.start_graphics(setup, self.proxy.clone());
                    Ok(())
                });
            if let Err(error) = result {
                self.session.fail(error);
            }
        }
        self.update_output();
        self.request_redraw();
        self.poll(event_loop);
    }
    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        self.suspended = true;
        self.update_output();
    }
    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: Event) {
        match event {
            Event::Wake => {
                self.wake.queued.store(false, Ordering::Release);
            }
            Event::GraphicsReady => {
                self.ready = true;
                self.request_redraw();
            }
            Event::Presented => {
                self.request_redraw();
            }
            Event::Stop => self.session.stop(),
        }
        self.poll(event_loop);
    }
    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(window) = &self.window else { return };
        if window.id() != id {
            return;
        }
        match event {
            WindowEvent::CloseRequested | WindowEvent::Destroyed => self.session.stop(),
            WindowEvent::Occluded(hidden) => {
                self.occluded = hidden;
                self.update_output();
                self.request_redraw();
            }
            WindowEvent::Resized(size) => {
                self.size = size.into();
                self.update_output();
                self.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                self.size = window.inner_size().into();
                self.update_output();
                self.request_redraw();
            }
            WindowEvent::RedrawRequested
                if self.ready && self.visible() && !self.session.closing() =>
            {
                self.session.redraw();
            }
            WindowEvent::KeyboardInput {
                event,
                is_synthetic: false,
                ..
            } => self.session.input(Input::Key(event)),
            WindowEvent::ModifiersChanged(modifiers) => {
                self.session.input(Input::Modifiers(modifiers.state()))
            }
            WindowEvent::Focused(focused) => self.session.input(Input::Focused(focused)),
            _ => {}
        }
        self.poll(event_loop);
    }
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        event_loop.set_control_flow(ControlFlow::Wait);
        self.poll(event_loop);
    }
    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = self.window.take() {
            platform::detach(&window);
        }
        self.session.finish();
    }
}

struct WakeEvents {
    proxy: EventLoopProxy<Event>,
    queued: AtomicBool,
}
impl Wake for WakeEvents {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        if !self.queued.swap(true, Ordering::AcqRel) {
            let _ = self.proxy.send_event(Event::Wake);
        }
    }
}
