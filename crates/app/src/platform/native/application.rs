use winit::{dpi::LogicalSize, window::WindowAttributes};

pub(crate) fn attributes(title: &str) -> Result<WindowAttributes, String> {
    Ok(WindowAttributes::default()
        .with_title(title)
        .with_inner_size(LogicalSize::new(640, 480)))
}

pub(crate) fn report(_: &Result<(), String>) {}

#[cfg(feature = "audio")]
pub(crate) fn start_audio(audio: &crate::audio::Audio) -> Result<(), cpal::Error> {
    audio.play()
}

#[cfg(feature = "audio")]
pub(crate) fn window_event(_: &mut Option<crate::audio::Audio>) {}

#[cfg(feature = "audio")]
pub(crate) fn report_audio_error(error: cpal::Error) {
    eprintln!("Audio unavailable: {error}");
}
