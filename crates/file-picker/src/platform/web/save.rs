use super::{browser_error, window};
use std::io;
use wasm_bindgen::prelude::*;
use web_sys::HtmlElement;

pub(crate) async fn save_file(name: &str, contents: Vec<u8>) -> io::Result<Option<()>> {
    let body = window()?
        .document()
        .and_then(|document| document.body())
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::Unsupported, "saving requires a document body")
        })?;
    download(&body, name, &contents).map_err(browser_error)?;
    Ok(Some(()))
}

#[wasm_bindgen(inline_js = r#"
export function download(body, name, bytes) {
    const link = body.ownerDocument.createElement("a");
    link.download = name;
    link.hidden = true;
    // Blob needs an ordinary ArrayBuffer and must outlive the borrowed Wasm bytes.
    const blob = new Blob([new Uint8Array(bytes)], { type: "application/octet-stream" });
    const url = URL.createObjectURL(blob);
    try {
        link.href = url;
        body.append(link);
        link.click();
    } finally {
        link.remove();
        // Browsers can resolve the URL after click returns and expose no completion event.
        setTimeout(() => URL.revokeObjectURL(url), 40_000);
    }
}
"#)]
extern "C" {
    #[wasm_bindgen(catch)]
    fn download(body: &HtmlElement, name: &str, bytes: &[u8]) -> Result<(), JsValue>;
}
