//! Windows browsers use message timers to avoid coarse atomic-wait timeouts.

use wasm_bindgen::prelude::*;

#[wasm_bindgen(inline_js = r#"
export function needsMessageTimer() {
    const navigator = globalThis.navigator;
    const platform = navigator?.userAgentData?.platform ?? navigator?.platform ?? "";
    return platform.startsWith("Win");
}
"#)]
extern "C" {
    // Unknown platforms keep Wasm parking, including macOS and Linux browsers.
    #[wasm_bindgen(js_name = needsMessageTimer)]
    pub fn needs_message_timer() -> bool;
}
