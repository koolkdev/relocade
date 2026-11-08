//! Standard 16×16 graphics masks over device-owned pixel locations.

use relocade_machine::{FarPointer, MemoryView};
use relocade_vga::{Framebuffer, Pixel, PixelFormat};

pub(super) struct Shape {
    hotspot: [i16; 2],
    screen: [u16; 16],
    cursor: [u16; 16],
}

pub(super) struct Background {
    // Only pixels changed by the 16×16 mask are retained; at most 256 entries.
    pixels: Vec<Pixel>,
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

    pub fn draw(&self, frame: &mut Framebuffer<'_>, position: [i32; 2]) -> Background {
        let left = position[0] - i32::from(self.hotspot[0]);
        let top = position[1] - i32::from(self.hotspot[1]);
        let mut pixels = Vec::new();
        for row in 0..16 {
            for column in 0..16 {
                let bit = 0x8000 >> column;
                let preserve = self.screen[row] & bit != 0;
                let invert = self.cursor[row] & bit != 0;
                if preserve && !invert {
                    continue;
                }
                let (Ok(x), Ok(y)) = ((left + column).try_into(), (top + row as i32).try_into())
                else {
                    continue;
                };
                let Some(pixel) = frame.pixel(x, y) else {
                    continue;
                };
                let screen = if preserve {
                    u32::MAX
                } else {
                    !pixel.color_mask()
                };
                let white = match pixel.format() {
                    PixelFormat::Indexed8 => pixel.color_mask().min(15),
                    PixelFormat::Rgb565 | PixelFormat::Bgrx8888 => pixel.color_mask(),
                };
                let cursor = if invert { white } else { 0 };
                frame.write_pixel(&pixel, (pixel.value() & screen) ^ cursor);
                pixels.push(pixel);
            }
        }
        Background { pixels }
    }
}

impl Background {
    pub fn restore(self, frame: &mut Framebuffer<'_>) {
        // CRTC counting and split layouts can alias pixels. Reverse the writes so
        // an alias cannot restore a value sampled after another cursor write.
        for pixel in self.pixels.into_iter().rev() {
            frame.write_pixel(&pixel, pixel.value());
        }
    }
}
