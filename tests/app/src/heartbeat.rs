use super::Observations;
use std::sync::{atomic::Ordering, Arc};
use wasm_bindgen::{closure::Closure, JsCast};

// A real worker event-loop task, independent of the Rust channel wakeups.
pub(super) struct Heartbeat {
    worker: web_sys::DedicatedWorkerGlobalScope,
    timer: i32,
    _callback: Closure<dyn FnMut()>,
}
impl Heartbeat {
    pub fn new(observations: Arc<Observations>) -> Self {
        let worker: web_sys::DedicatedWorkerGlobalScope = js_sys::global().unchecked_into();
        let callback = Closure::wrap(Box::new(move || {
            observations.worker_tasks.fetch_add(1, Ordering::Release);
        }) as Box<dyn FnMut()>);
        let timer = worker
            .set_interval_with_callback_and_timeout_and_arguments_0(
                callback.as_ref().unchecked_ref(),
                10,
            )
            .unwrap();
        Self {
            worker,
            timer,
            _callback: callback,
        }
    }
}
impl Drop for Heartbeat {
    fn drop(&mut self) {
        self.worker.clear_interval_with_handle(self.timer);
    }
}
