use super::{host::Event, Control};
use wasm_bindgen::prelude::*;
use winit::event_loop::EventLoopProxy;

#[wasm_bindgen]
pub struct BrowserControl {
    proxy: EventLoopProxy<Event>,
    finished: js_sys::Promise,
}
#[wasm_bindgen]
impl BrowserControl {
    pub fn stop(&self) {
        let _ = self.proxy.send_event(Event::Stop);
    }
    #[wasm_bindgen(getter)]
    pub fn finished(&self) -> js_sys::Promise {
        self.finished.clone()
    }
}

pub fn control<T: Into<JsValue> + 'static>(control: Control<T>) -> BrowserControl {
    let proxy = control.proxy.clone();
    let finished = wasm_bindgen_futures::future_to_promise(async move {
        control
            .await
            .map(|result| result.map(Into::into).unwrap_or(JsValue::UNDEFINED))
            .map_err(|error| js_sys::Error::new(&error).into())
    });
    BrowserControl { proxy, finished }
}
