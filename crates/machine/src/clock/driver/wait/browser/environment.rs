//! Limit the alternative wait to the environment with measured coarse atomic waits.

use wasm_bindgen::prelude::*;

#[wasm_bindgen(inline_js = r#"
export function needsMessageTimer() {
    const navigator = globalThis.navigator;
    const platform = navigator?.userAgentData?.platform ?? navigator?.platform ?? "";
    const chromium = navigator?.userAgentData?.brands?.some(
        ({ brand }) => brand === "Chromium" || brand === "Google Chrome"
    ) || /(?:Chrome|Chromium)\//.test(navigator?.userAgent ?? "");
    return platform.startsWith("Win") && chromium;
}
"#)]
extern "C" {
    // Unknown platforms keep Wasm parking, including macOS and Linux browsers.
    #[wasm_bindgen(js_name = needsMessageTimer)]
    pub fn needs_message_timer() -> bool;
}
