use wasm_bindgen::JsCast;
use winit::{platform::web::WindowAttributesExtWebSys, window::WindowAttributes};

pub(crate) fn attributes(title: &str) -> Result<WindowAttributes, String> {
    let document = web_sys::window()
        .expect("application runs on main")
        .document()
        .unwrap();
    document.set_title(title);
    if let Some(heading) = document.get_element_by_id("title") {
        heading.set_text_content(Some(title));
    }
    let canvas = document
        .get_element_by_id("screen")
        .map(|element| {
            element
                .dyn_into::<web_sys::HtmlCanvasElement>()
                .map_err(|_| "screen must be a canvas".to_string())
        })
        .transpose()?;
    Ok(WindowAttributes::default()
        .with_title(title)
        .with_canvas(canvas))
}

pub(crate) fn report(result: &Result<(), String>) {
    if let Err(error) = result {
        web_sys::console::error_1(&error.as_str().into());
    }
}
