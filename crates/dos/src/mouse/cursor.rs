//! Cursor visibility and ownership of the background saved by each drawing mode.

mod graphics;
mod text;

use super::state::Point;
use relocade_bios::video;
use relocade_machine::{FarPointer, IoBus, MemoryView};

pub(super) struct Cursor {
    visibility: i16,
    text: text::Shape,
    graphics: graphics::Shape,
    saved: Option<Background>,
}

enum Background {
    Text(text::Background),
    Graphics(graphics::Background),
}

impl Default for Cursor {
    fn default() -> Self {
        Self {
            visibility: -1,
            text: text::Shape::default(),
            graphics: graphics::Shape::default(),
            saved: None,
        }
    }
}

impl Cursor {
    pub fn reset(&mut self, memory: &mut MemoryView<'_>, io: &IoBus) {
        self.restore(memory, io);
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

    pub fn update(&mut self, memory: &mut MemoryView<'_>, io: &IoBus, position: Point) {
        self.restore(memory, io);
        if self.visibility != 0 {
            return;
        }
        self.saved = if video::mode(memory).mode & 0x7f == 0x13 {
            Some(Background::Graphics(
                self.graphics.draw(memory, io, position),
            ))
        } else {
            self.text.draw(memory, position).map(Background::Text)
        };
    }

    fn restore(&mut self, memory: &mut MemoryView<'_>, io: &IoBus) {
        match self.saved.take() {
            Some(Background::Text(saved)) => saved.restore(memory),
            Some(Background::Graphics(saved)) => saved.restore(memory, io),
            None => {}
        }
    }
}
