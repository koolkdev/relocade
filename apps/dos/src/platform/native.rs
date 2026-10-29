use relocade_app::winit::{dpi::LogicalSize, window::WindowAttributes};

pub(super) fn attributes() -> WindowAttributes {
    WindowAttributes::default()
        .with_title("Relocade DOS")
        .with_inner_size(LogicalSize::new(800, 600))
}

pub(super) fn report_error(error: &str) {
    eprintln!("Relocade DOS: {error}");
}
