use relocade_app::{
    winit::{
        application::ApplicationHandler,
        dpi::PhysicalSize,
        event::WindowEvent,
        event_loop::{ActiveEventLoop, EventLoop},
        platform::x11::EventLoopBuilderExtX11,
        window::{WindowAttributes, WindowId},
    },
    Window, WindowState,
};

#[derive(Default)]
struct App {
    window: Option<Window>,
    resized: Option<WindowState>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window = Window::new(
            event_loop,
            WindowAttributes::default().with_inner_size(PhysicalSize::new(640, 480)),
        )
        .unwrap();
        assert!(window.state().drawable());
        let _ = window
            .winit()
            .request_inner_size(PhysicalSize::new(512, 320));
        self.window = Some(window);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(window) = &mut self.window else {
            return;
        };
        if !window.handle_event(id, &event) {
            return;
        }
        if matches!(event, WindowEvent::Resized(size) if size == PhysicalSize::new(512, 320)) {
            self.resized = Some(window.state());
            window.handle_event(id, &WindowEvent::Occluded(true));
            assert!(!window.state().drawable());
            window.handle_event(id, &WindowEvent::Occluded(false));
            assert!(window.state().drawable());
            window.handle_event(id, &WindowEvent::CloseRequested);
            assert!(window.state().close_requested);
            assert!(!window.state().drawable());
            event_loop.exit();
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, (): ()) {
        event_loop.exit(); // Bound a fixture failure if no resize arrives.
    }

    fn exiting(&mut self, _: &ActiveEventLoop) {
        self.window = None;
    }
}

#[test]
#[ignore = "requires X11; run under Xvfb"]
fn native_window_tracks_resize_and_close() {
    let event_loop = EventLoop::builder()
        .with_x11()
        .with_any_thread(true)
        .build()
        .unwrap();
    let proxy = event_loop.create_proxy();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(10));
        let _ = proxy.send_event(());
    });
    let mut app = App::default();
    event_loop.run_app(&mut app).unwrap();
    let state = app.resized.expect("resize reached the window owner");
    assert_eq!(state.size, PhysicalSize::new(512, 320));
    assert_eq!(state.scale_factor, 1.0);
    assert!(app.window.is_none());
}
