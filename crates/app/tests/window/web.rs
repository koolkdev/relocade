use futures_channel::oneshot;
use relocade_app::{
    winit::{
        application::ApplicationHandler,
        event::WindowEvent,
        event_loop::ActiveEventLoop,
        platform::web::WindowAttributesExtWebSys,
        window::{WindowAttributes, WindowId},
    },
    Window, WindowState,
};
use wasm_bindgen::JsCast;

#[derive(Debug)]
enum Command {
    Close,
    Finish,
}

struct App {
    canvas: web_sys::HtmlCanvasElement,
    window: Option<Window>,
    ready: Option<oneshot::Sender<WindowState>>,
    resized: Option<oneshot::Sender<WindowState>>,
    close_requested: Option<oneshot::Sender<WindowState>>,
    closed: Option<oneshot::Sender<()>>,
}

impl ApplicationHandler<Command> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.window = Some(
            Window::new(
                event_loop,
                WindowAttributes::default().with_canvas(Some(self.canvas.clone())),
            )
            .unwrap(),
        );
        self.ready
            .take()
            .unwrap()
            .send(self.window.as_ref().unwrap().state())
            .unwrap();
    }

    fn window_event(&mut self, _: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(window) = &mut self.window else {
            return;
        };
        if !window.handle_event(id, &event) {
            return;
        }
        let state = window.state();
        if matches!(event, WindowEvent::Resized(_))
            && state.size.width == (512.0 * state.scale_factor) as u32
        {
            if let Some(resized) = self.resized.take() {
                resized.send(state).unwrap();
            }
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, command: Command) {
        match command {
            Command::Close => {
                let window = self.window.as_mut().unwrap();
                window.handle_event(window.winit().id(), &WindowEvent::CloseRequested);
                self.close_requested
                    .take()
                    .unwrap()
                    .send(window.state())
                    .unwrap();
            }
            Command::Finish => event_loop.exit(),
        }
    }

    fn exiting(&mut self, _: &ActiveEventLoop) {
        self.window = None;
        self.closed.take().unwrap().send(()).unwrap();
    }
}

#[wasm_bindgen_test::wasm_bindgen_test]
async fn canvas_attaches_follows_css_and_detaches_on_close() {
    let document = web_sys::window().unwrap().document().unwrap();
    let canvas: web_sys::HtmlCanvasElement = document
        .create_element("canvas")
        .unwrap()
        .dyn_into()
        .unwrap();
    canvas.style().set_property("width", "640px").unwrap();
    canvas.style().set_property("height", "480px").unwrap();
    let (ready, started) = oneshot::channel();
    let (resized, changed) = oneshot::channel();
    let (close_requested, closing) = oneshot::channel();
    let (closed, finished) = oneshot::channel();
    let mut events = None;
    relocade_app::run(|proxy| {
        events = Some(proxy);
        App {
            canvas: canvas.clone(),
            window: None,
            ready: Some(ready),
            resized: Some(resized),
            close_requested: Some(close_requested),
            closed: Some(closed),
        }
    })
    .unwrap();
    let initial = started.await.unwrap();
    assert!(canvas.is_connected());
    assert_eq!((canvas.client_width(), canvas.client_height()), (640, 480));
    assert_eq!(
        initial.scale_factor,
        web_sys::window().unwrap().device_pixel_ratio()
    );
    canvas.style().set_property("width", "512px").unwrap();
    canvas.style().set_property("height", "320px").unwrap();
    let resized = changed.await.unwrap();
    assert_eq!(resized.size.height, (320.0 * resized.scale_factor) as u32);
    assert!(!resized.close_requested);
    let events = events.unwrap();
    events.send_event(Command::Close).unwrap();
    let closing = closing.await.unwrap();
    assert!(closing.close_requested);
    assert!(!closing.drawable());
    assert!(
        canvas.is_connected(),
        "close waits for the application's shutdown"
    );
    events.send_event(Command::Finish).unwrap();
    finished.await.unwrap();
    assert!(!canvas.is_connected());
}
