use crate::js_error;
use relocade_runtime::Result;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

#[wasm_bindgen(module = "/src/scheduling.js")]
extern "C" {
    #[wasm_bindgen(js_name = nextTask)]
    fn next_task() -> js_sys::Promise;
}

/// Lets the current worker or window event loop process tasks before resuming.
///
/// A CPU worker calls this between budgeted guest runs so input, control and
/// compiler messages can be delivered. Awaiting an already-ready future or
/// repeatedly chaining Promise microtasks does not provide that opportunity.
/// The embedding chooses the execution interval; `Runtime::run_async` does not
/// implicitly yield. This does not wait for any particular message to arrive.
pub async fn yield_to_event_loop() -> Result<()> {
    JsFuture::from(next_task())
        .await
        .map(|_| ())
        .map_err(|error| js_error("yield to event loop", error))
}
