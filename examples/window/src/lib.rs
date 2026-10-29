//! A main-thread window/canvas with input, resize and close callbacks.
use relocade_app::winit::{
    application::ApplicationHandler,
    error::EventLoopError,
    event::{ElementState, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoopProxy},
    keyboard::{Key, NamedKey},
    window::{Window, WindowId},
};
use std::{cell::Cell, rc::Rc};

enum Command {
    Close,
}

struct WindowApp {
    window: Option<Window>,
    events: EventLoopProxy<Command>,
    redraws: Rc<Cell<u32>>,
    key: String,
}

impl ApplicationHandler<Command> for WindowApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = Window::default_attributes().with_title("Relocade window");
        #[cfg(not(target_arch = "wasm32"))]
        let attributes =
            attributes.with_inner_size(relocade_app::winit::dpi::LogicalSize::new(640, 480));
        #[cfg(target_arch = "wasm32")]
        let attributes = {
            use relocade_app::winit::platform::web::WindowAttributesExtWebSys;
            // The embedding's CSS owns canvas dimensions.
            attributes.with_append(true)
        };
        let window = event_loop
            .create_window(attributes)
            .expect("create example window");
        window.request_redraw();
        self.window = Some(window);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(window) = &self.window else { return };
        if id != window.id() {
            return;
        }
        match event {
            WindowEvent::CloseRequested | WindowEvent::Destroyed => event_loop.exit(),
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                window.request_redraw()
            }
            WindowEvent::KeyboardInput {
                event,
                is_synthetic: false,
                ..
            } if event.state == ElementState::Pressed => {
                match event.logical_key {
                    Key::Named(NamedKey::Escape) => {
                        // Application-defined notifications return through winit on main.
                        let _ = self.events.send_event(Command::Close);
                    }
                    Key::Character(key) => self.key = key.to_string(),
                    _ => {}
                }
                window.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                self.redraws.set(self.redraws.get() + 1);
                let size = window.inner_size();
                let title = format!(
                    "Window {}x{} | key={} | redraws={}",
                    size.width,
                    size.height,
                    self.key,
                    self.redraws.get()
                );
                window.set_title(&title);
                #[cfg(target_arch = "wasm32")]
                status(&title);
            }
            _ => {}
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, Command::Close: Command) {
        event_loop.exit();
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(_window) = self.window.take() {
            #[cfg(target_arch = "wasm32")]
            {
                use relocade_app::winit::platform::web::WindowExtWebSys;
                if let Some(canvas) = _window.canvas() {
                    canvas.remove();
                }
                status("Closed");
            }
        }
    }
}

pub fn run() -> Result<(), EventLoopError> {
    // Shared with the native caller after exit; the application is intentionally !Send.
    let redraws = Rc::new(Cell::new(0));
    relocade_app::run(|events| WindowApp {
        window: None,
        events,
        redraws: redraws.clone(),
        key: "-".into(),
    })?;
    #[cfg(not(target_arch = "wasm32"))]
    println!("Window closed after {} redraws", redraws.get());
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn status(message: &str) {
    web_sys::window()
        .unwrap()
        .document()
        .unwrap()
        .get_element_by_id("output")
        .unwrap()
        .set_text_content(Some(message));
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() -> Result<(), wasm_bindgen::JsValue> {
    run().map_err(|error| wasm_bindgen::JsValue::from_str(&error.to_string()))
}
