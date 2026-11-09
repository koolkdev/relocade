use winit::window::{CursorGrabMode, Window};

#[derive(Default)]
pub(crate) struct MouseCapture {
    captured: bool,
}

impl MouseCapture {
    pub(crate) fn new(_: &Window, _: impl Fn() + 'static) -> Result<Self, String> {
        Ok(Self::default())
    }

    pub(crate) fn request(&mut self, window: &Window) {
        self.captured = window
            .set_cursor_grab(CursorGrabMode::Locked)
            .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined))
            .is_ok();
        window.set_cursor_visible(!self.captured);
    }

    pub(crate) fn requested(&self) -> bool {
        self.captured
    }

    pub(crate) fn captured(&self) -> bool {
        self.captured
    }

    pub(crate) fn release(&mut self, window: &Window) {
        self.captured = false;
        let _ = window.set_cursor_grab(CursorGrabMode::None);
        window.set_cursor_visible(true);
    }
}
