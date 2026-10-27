//! Browser control and completion conversion for application entrypoints.
use crate::{Control, Exit, StopHandle};
use wasm_bindgen::{prelude::wasm_bindgen, JsValue};

/// A browser application's shutdown request and completion promise.
/// Returned by an application's export; importing bindings never starts work.
#[wasm_bindgen]
pub struct BrowserControl {
    stop: StopHandle,
    finished: js_sys::Promise,
}

/// Converts an application launch into a browser control. The application decides
/// how its typed completion or cancellation is represented in JavaScript.
/// Launch errors and rejected completion promises carry JavaScript Error values.
pub fn launch<T: 'static>(
    control: Result<Control<T>, String>,
    map_exit: impl FnOnce(Exit<T>) -> JsValue + 'static,
) -> Result<BrowserControl, JsValue> {
    let control = control.map_err(js_error)?;
    let stop = control.stop_handle();
    let finished = wasm_bindgen_futures::future_to_promise(async move {
        control.await.map(map_exit).map_err(js_error)
    });
    Ok(BrowserControl { stop, finished })
}

#[wasm_bindgen]
impl BrowserControl {
    /// Requests shutdown even when application work is awaiting input.
    pub fn stop(&self) {
        self.stop.stop();
    }

    /// Settles after the application closes and its worker tasks finish.
    #[wasm_bindgen(getter)]
    pub fn finished(&self) -> js_sys::Promise {
        self.finished.clone()
    }
}

fn js_error(error: String) -> JsValue {
    js_sys::Error::new(&error).into()
}

#[cfg(test)]
#[path = "web/tests.rs"]
mod tests;
