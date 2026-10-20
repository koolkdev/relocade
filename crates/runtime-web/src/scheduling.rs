use crate::js_error;
use relocade_runtime::Result;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

#[wasm_bindgen(module = "/src/scheduling.js")]
extern "C" {
    #[wasm_bindgen(js_name = yieldExecution)]
    fn yield_execution() -> js_sys::Promise;
}

pub(super) async fn yield_task() -> Result<()> {
    JsFuture::from(yield_execution())
        .await
        .map(|_| ())
        .map_err(|error| js_error("yield execution", error))
}
