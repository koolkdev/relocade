use wasm_bindgen::{closure::Closure, JsCast};
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
        web_sys::console::error_1(&error.clone().into());
        if let Some(element) = web_sys::window()
            .unwrap()
            .document()
            .unwrap()
            .get_element_by_id("error")
        {
            element.set_text_content(Some(error));
        }
    }
}

/// Own the optional browser Close control for exactly the application lifetime.
pub(crate) struct Controls(Option<(web_sys::HtmlButtonElement, Closure<dyn FnMut()>)>);

impl Controls {
    pub(crate) fn new(close: impl Fn() + 'static) -> Self {
        let button = web_sys::window()
            .unwrap()
            .document()
            .unwrap()
            .get_element_by_id("close")
            .and_then(|element| element.dyn_into::<web_sys::HtmlButtonElement>().ok());
        Self(button.map(|button| {
            button.set_disabled(false);
            let callback = Closure::new(close);
            button
                .add_event_listener_with_callback("click", callback.as_ref().unchecked_ref())
                .expect("close event listener");
            (button, callback)
        }))
    }
}

impl Drop for Controls {
    fn drop(&mut self) {
        if let Some((button, callback)) = &self.0 {
            let _ = button
                .remove_event_listener_with_callback("click", callback.as_ref().unchecked_ref());
            button.set_disabled(true);
        }
    }
}
