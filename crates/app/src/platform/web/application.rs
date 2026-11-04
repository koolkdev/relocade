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

#[cfg(feature = "audio")]
pub(crate) fn start_audio(audio: &crate::audio::Audio) -> Result<(), cpal::Error> {
    // Sticky activation survives winit's buffered delivery and worklet startup.
    if web_sys::window()
        .expect("application runs on main")
        .navigator()
        .user_activation()
        .has_been_active()
    {
        audio.play()?;
    }
    Ok(())
}

#[cfg(feature = "audio")]
pub(crate) fn window_event(output: &mut Option<crate::audio::Audio>) {
    if let Some(audio) = output {
        if let Err(error) = start_audio(audio) {
            *output = None;
            report_audio_error(error);
        }
    }
}

#[cfg(feature = "audio")]
pub(crate) fn report_audio_error(error: cpal::Error) {
    web_sys::console::warn_1(&format!("Audio unavailable: {error}").into());
}
