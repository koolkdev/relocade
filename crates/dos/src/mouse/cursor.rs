//! Cursor visibility and ownership of the background saved by each drawing mode.

pub(super) mod graphics;
pub(super) mod text;

use relocade_machine::{FarPointer, MemoryView};
use relocade_vga::{Framebuffer, FramebufferGeometry};

pub(super) struct Cursor {
    pub(super) visibility: i16,
    pub(super) text: text::Shape,
    pub(super) graphics: graphics::Shape,
    saved: Option<Background>,
}

enum Background {
    Text(text::Background),
    Graphics(graphics::Background),
}

impl Default for Cursor {
    fn default() -> Self {
        Self::from_state(-1, text::Shape::default(), graphics::Shape::default())
    }
}

impl Cursor {
    pub(super) fn from_state(
        visibility: i16,
        text: text::Shape,
        graphics: graphics::Shape,
    ) -> Self {
        Self {
            visibility,
            text,
            graphics,
            saved: None,
        }
    }

    pub fn reset(&mut self, frame: &mut Framebuffer<'_>) {
        self.restore(frame);
        *self = Self::default();
    }

    pub fn visibility(&mut self, show: bool) {
        self.visibility = if show {
            (self.visibility + 1).min(0)
        } else {
            self.visibility.saturating_sub(1)
        };
    }

    pub fn set_text_mask(&mut self, screen: u16, cursor: u16) {
        self.text = text::Shape { screen, cursor };
    }

    pub fn set_graphics_mask(
        &mut self,
        memory: &MemoryView<'_>,
        source: FarPointer,
        hotspot: [i16; 2],
    ) {
        self.graphics = graphics::Shape::read(memory, source, hotspot);
    }

    pub fn update(&mut self, frame: &mut Framebuffer<'_>, position: [i32; 2]) {
        self.restore(frame);
        if self.visibility != 0 {
            return;
        }
        self.saved = match frame.geometry() {
            Some(FramebufferGeometry::Graphics { .. }) => {
                Some(Background::Graphics(self.graphics.draw(frame, position)))
            }
            Some(FramebufferGeometry::Text { .. }) => {
                self.text.draw(frame, position).map(Background::Text)
            }
            None => None,
        };
    }

    fn restore(&mut self, frame: &mut Framebuffer<'_>) {
        match self.saved.take() {
            Some(Background::Text(saved)) => saved.restore(frame),
            Some(Background::Graphics(saved)) => saved.restore(frame),
            None => {}
        }
    }
}
