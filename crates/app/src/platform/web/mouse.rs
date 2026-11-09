use std::{cell::Cell, rc::Rc};
use wasm_bindgen::{closure::Closure, JsCast};
use winit::{platform::web::WindowExtWebSys, window::Window};

/// Owns the browser subscription as well as any pending pointer-lock request.
/// A successful request is asynchronous; the document is the capture authority.
pub(crate) struct MouseCapture {
    document: web_sys::Document,
    canvas: web_sys::HtmlCanvasElement,
    requested: Rc<Cell<bool>>,
    changed: Closure<dyn FnMut()>,
}

impl MouseCapture {
    pub(crate) fn new(window: &Window, notify: impl Fn() + 'static) -> Result<Self, String> {
        let document = web_sys::window().unwrap().document().unwrap();
        let canvas = window.canvas().expect("application window has a canvas");
        let requested = Rc::new(Cell::new(false));
        let changed = Closure::new({
            let document = document.clone();
            let canvas = canvas.clone();
            let requested = requested.clone();
            move || {
                // Focus loss or shutdown may cancel a request before the browser grants it.
                if !requested.get()
                    && document.pointer_lock_element().as_ref() == Some(canvas.as_ref())
                {
                    document.exit_pointer_lock();
                }
                if requested.get() {
                    notify();
                }
            }
        });
        let capture = Self {
            document,
            canvas,
            requested,
            changed,
        };
        for event in ["pointerlockchange", "pointerlockerror"] {
            capture
                .document
                .add_event_listener_with_callback(event, capture.changed.as_ref().unchecked_ref())
                .map_err(|error| format!("Cannot listen for pointer lock: {error:?}"))?;
        }
        Ok(capture)
    }

    pub(crate) fn request(&mut self, _: &Window) {
        if !self.requested.replace(true) {
            self.canvas.request_pointer_lock();
        }
    }

    pub(crate) fn requested(&self) -> bool {
        self.requested.get()
    }

    pub(crate) fn captured(&self) -> bool {
        self.requested.get()
            && self.document.pointer_lock_element().as_ref() == Some(self.canvas.as_ref())
    }

    pub(crate) fn release(&mut self, _: &Window) {
        self.cancel();
    }

    fn cancel(&self) {
        self.requested.set(false);
        if self.document.pointer_lock_element().as_ref() == Some(self.canvas.as_ref()) {
            self.document.exit_pointer_lock();
        }
    }
}

impl Drop for MouseCapture {
    fn drop(&mut self) {
        self.cancel();
        for event in ["pointerlockchange", "pointerlockerror"] {
            let _ = self
                .document
                .remove_event_listener_with_callback(event, self.changed.as_ref().unchecked_ref());
        }
    }
}
