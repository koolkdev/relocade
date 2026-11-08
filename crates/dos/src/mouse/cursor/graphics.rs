//! Standard 16×16 graphics masks, with Mode 13h pixel access owned by BIOS.

use super::Point;
use relocade_bios::video;
use relocade_machine::{FarPointer, IoBus, MemoryView};

pub(super) struct Shape {
    hotspot: [i16; 2],
    screen: [u16; 16],
    cursor: [u16; 16],
}

pub(super) struct Background {
    // Only pixels changed by the 16×16 mask are retained; at most 256 entries.
    pixels: Vec<Pixel>,
}

struct Pixel {
    x: u16,
    y: u16,
    color: u8,
}

impl Default for Shape {
    fn default() -> Self {
        Self {
            hotspot: [0, 0],
            screen: [
                0x3fff, 0x1fff, 0x0fff, 0x07ff, 0x03ff, 0x01ff, 0x00ff, 0x007f, 0x003f, 0x001f,
                0x01ff, 0x10ff, 0x30ff, 0xf87f, 0xf87f, 0xfcff,
            ],
            cursor: [
                0, 0x4000, 0x6000, 0x7000, 0x7800, 0x7c00, 0x7e00, 0x7f00, 0x7f80, 0x7c00, 0x6c00,
                0x4600, 0x0600, 0x0300, 0x0300, 0,
            ],
        }
    }
}

impl Shape {
    pub fn read(memory: &MemoryView<'_>, source: FarPointer, hotspot: [i16; 2]) -> Self {
        let mut bytes = [0; 64];
        memory
            .read_wrapping(source, &mut bytes)
            .expect("graphics cursor masks");
        let words: [u16; 32] = std::array::from_fn(|index| {
            u16::from_le_bytes([bytes[index * 2], bytes[index * 2 + 1]])
        });
        Self {
            hotspot,
            screen: words[..16].try_into().unwrap(),
            cursor: words[16..].try_into().unwrap(),
        }
    }

    pub fn draw(&self, memory: &mut MemoryView<'_>, io: &IoBus, position: Point) -> Background {
        // Mode 13h has a 640-unit logical X range and a 320-pixel display.
        let left = i32::from(position.x as i16) / 2 - i32::from(self.hotspot[0]);
        let top = i32::from(position.y as i16) - i32::from(self.hotspot[1]);
        let mut pixels = Vec::new();
        for row in 0..16 {
            for column in 0..16 {
                let x = left + column;
                let y = top + row as i32;
                if !(0..320).contains(&x) || !(0..200).contains(&y) {
                    continue;
                }
                let bit = 0x8000 >> column;
                let screen = if self.screen[row] & bit != 0 { 0xff } else { 0 };
                // White is palette index 15; XOR retains the background's upper nibble.
                let cursor = if self.cursor[row] & bit != 0 { 0x0f } else { 0 };
                if screen == 0xff && cursor == 0 {
                    continue;
                }
                let (x, y) = (x as u16, y as u16);
                let color = video::read_pixel(memory, io, 0, x, y).expect("Mode 13h pixel");
                video::write_pixel(memory, io, 0, x, y, (color & screen) ^ cursor);
                pixels.push(Pixel { x, y, color });
            }
        }
        Background { pixels }
    }
}

impl Background {
    pub fn restore(self, memory: &mut MemoryView<'_>, io: &IoBus) {
        if video::mode(memory).mode & 0x7f != 0x13 {
            return;
        }
        for pixel in self.pixels {
            video::write_pixel(memory, io, 0, pixel.x, pixel.y, pixel.color);
        }
    }
}
