use crate::application::{Application, Event};
use relocade_app::winit::{event_loop::EventLoopProxy, window::WindowAttributes};
use std::{cell::RefCell, rc::Rc};
use wasm_bindgen::prelude::*;

pub(super) fn attributes() -> WindowAttributes {
    // The embedding's CSS determines canvas dimensions.
    WindowAttributes::default().with_title("Relocade DOS")
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console, js_name = error)]
    fn console_error(error: &str);
}

pub(super) fn report_error(error: &str) {
    console_error(error);
}

/// Browser control remains on main. Closing drains workers before removing canvas.
#[wasm_bindgen]
pub struct DosWindow {
    events: EventLoopProxy<Event>,
    error: Rc<RefCell<Option<String>>>,
}

#[wasm_bindgen]
impl DosWindow {
    pub fn close(&self) {
        let _ = self.events.send_event(Event::Close);
    }
    pub fn error(&self) -> Option<String> {
        self.error.borrow().clone()
    }
}

#[wasm_bindgen]
pub fn open(program: &[u8]) -> Result<DosWindow, JsValue> {
    console_error_panic_hook::set_once();
    let error = Rc::new(RefCell::new(None));
    let mut control = None;
    relocade_app::run(|events| {
        control = Some(DosWindow {
            events: events.clone(),
            error: error.clone(),
        });
        Application::new(program.to_vec(), events, error)
    })
    .map_err(|e| JsValue::from_str(&e.to_string()))?;
    Ok(control.expect("application factory runs before returning"))
}
