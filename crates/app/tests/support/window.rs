use futures_channel::oneshot;
use relocade_app::{
    graphics::SurfaceSetup,
    winit::{
        application::ApplicationHandler,
        dpi::PhysicalSize,
        event::WindowEvent,
        event_loop::{ActiveEventLoop, EventLoopProxy},
        window::{WindowAttributes, WindowId},
    },
    Window,
};

use std::future::Future;

pub struct Ready {
    pub setup: SurfaceSetup,
    pub size: PhysicalSize<u32>,
    pub events: EventLoopProxy<()>,
    #[cfg(target_os = "linux")]
    pub window_id: u32,
}

struct App {
    window: Option<Window>,
    events: EventLoopProxy<()>,
    ready: Option<oneshot::Sender<Ready>>,
    closed: Option<oneshot::Sender<()>>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window = Window::new(event_loop, WindowAttributes::default()).unwrap();
        window.winit().request_redraw();
        self.window = Some(window);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let window = self.window.as_mut().unwrap();
        if !window.handle_event(id, &event) {
            return;
        }
        if matches!(event, WindowEvent::RedrawRequested) && window.state().drawable() {
            if let Some(ready) = self.ready.take() {
                let setup = window.surface_setup(event_loop).unwrap();
                ready
                    .send(Ready {
                        setup,
                        size: window.state().size,
                        events: self.events.clone(),
                        #[cfg(target_os = "linux")]
                        window_id: native::window_id(window),
                    })
                    .ok()
                    .unwrap();
            }
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, (): ()) {
        event_loop.exit();
    }

    fn exiting(&mut self, _: &ActiveEventLoop) {
        self.window = None;
        self.closed.take().unwrap().send(()).unwrap();
    }
}

#[cfg(target_os = "linux")]
#[path = "../surface/native.rs"]
pub mod native;

pub struct TestWindow {
    pub ready: Ready,
    closed: oneshot::Receiver<()>,
    #[cfg(target_os = "linux")]
    host: std::thread::JoinHandle<()>,
}

impl TestWindow {
    pub async fn open() -> Self {
        let (ready, started) = oneshot::channel();
        let (closed, finished) = oneshot::channel();
        #[cfg(target_os = "linux")]
        let host = std::thread::spawn(move || {
            use relocade_app::winit::{
                event_loop::EventLoop, platform::x11::EventLoopBuilderExtX11,
            };
            let event_loop = EventLoop::builder()
                .with_x11()
                .with_any_thread(true)
                .build()
                .unwrap();
            let mut app = App {
                window: None,
                events: event_loop.create_proxy(),
                ready: Some(ready),
                closed: Some(closed),
            };
            event_loop.run_app(&mut app).unwrap();
        });
        #[cfg(target_arch = "wasm32")]
        relocade_app::run(|events| App {
            window: None,
            events,
            ready: Some(ready),
            closed: Some(closed),
        })
        .unwrap();
        Self {
            ready: started.await.unwrap(),
            closed: finished,
            #[cfg(target_os = "linux")]
            host,
        }
    }

    pub async fn close(events: EventLoopProxy<()>, closed: oneshot::Receiver<()>) {
        events.send_event(()).unwrap();
        closed.await.unwrap();
    }

    pub fn split(self) -> (Ready, impl Future<Output = ()>) {
        let Self {
            ready,
            closed,
            #[cfg(target_os = "linux")]
            host,
        } = self;
        let events = ready.events.clone();
        (ready, async move {
            Self::close(events, closed).await;
            #[cfg(target_os = "linux")]
            host.join().unwrap();
        })
    }
}
