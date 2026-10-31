use winit::{dpi::LogicalSize, window::WindowAttributes};

pub(crate) fn attributes(title: &str) -> Result<WindowAttributes, String> {
    Ok(WindowAttributes::default()
        .with_title(title)
        .with_inner_size(LogicalSize::new(640, 480)))
}

pub(crate) fn report(_: &Result<(), String>) {}

pub(crate) struct Controls;
impl Controls {
    pub(crate) fn new(_: impl Fn() + 'static) -> Self {
        Self
    }
}
