//! Native drawing into guest VRAM, independent of CPU aperture programming.
mod addressing;
mod location;
#[cfg(test)]
mod tests;

use crate::{memory::VideoMemory, PixelFormat, Scanout};
use addressing::Addressing;
use location::Location;

/// The character grid or graphics pixels addressed by native drawing services.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FramebufferGeometry {
    Text { columns: u16, rows: u16 },
    Graphics { width: u16, height: u16 },
}

/// Clock-independent access to the execution owner's video memory.
/// A scope never survives a guest call or a wait.
pub trait FramebufferAccess {
    fn framebuffer(&mut self) -> Framebuffer<'_>;
}

/// A synchronous drawing scope over physical VRAM. Writes participate in normal
/// capture dirty tracking. Registers, CPU latches and bank selection are untouched.
pub struct Framebuffer<'a> {
    pub(crate) memory: &'a mut VideoMemory,
    pub(crate) scanout: Scanout,
}

/// A sampled pixel and its original physical location. Retaining this value lets
/// a service restore the original page even after the viewport moves.
#[derive(Clone, Copy)]
pub struct Pixel {
    location: Location,
    value: u32,
    color_mask: u32,
}

impl Pixel {
    pub fn value(&self) -> u32 {
        self.value
    }

    /// Color bits; other bits, such as BGRX's reserved byte, can be preserved.
    pub fn color_mask(&self) -> u32 {
        self.color_mask
    }

    /// Planar and serialized VGA colors are palette indices.
    pub fn format(&self) -> PixelFormat {
        match self.location {
            Location::Packed { format, .. } => format,
            _ => PixelFormat::Indexed8,
        }
    }
}

/// A character/attribute word and its original physical plane address.
#[derive(Clone, Copy)]
pub struct TextCell {
    address: u32,
    value: u16,
}

impl TextCell {
    pub fn value(&self) -> u16 {
        self.value
    }
}

impl Framebuffer<'_> {
    pub fn geometry(&self) -> Option<FramebufferGeometry> {
        match &self.scanout {
            Scanout::Vga(state) => Some(Addressing::new(state)?.geometry()),
            Scanout::Packed(layout) => {
                let [width, height] = layout.size().map(|n| n as u16);
                Some(FramebufferGeometry::Graphics { width, height })
            }
        }
    }

    /// Sample a graphics pixel before palette lookup. Coordinates are relative
    /// to the programmed viewport; out-of-bounds or text requests return None.
    pub fn pixel(&self, x: u16, y: u16) -> Option<Pixel> {
        let (location, color_mask) = match &self.scanout {
            Scanout::Vga(state) => {
                let addressing = Addressing::new(state)?;
                let location = addressing.pixel(x, y)?;
                (location, state.parameters.color_mask)
            }
            Scanout::Packed(layout) => {
                let [width, height] = layout.size();
                if u32::from(x) >= width || u32::from(y) >= height {
                    return None;
                }
                let format = layout.format();
                let address = layout.pixel_address(u32::from(x), u32::from(y));
                (Location::Packed { address, format }, layout.color_mask())
            }
        };
        Some(Pixel {
            value: location.read(self.memory),
            location,
            color_mask,
        })
    }

    /// Replace the sampled pixel at its recorded location, retaining neighboring
    /// pixels in packed/planar bytes. This does not resample its original value.
    pub fn write_pixel(&mut self, pixel: &Pixel, value: u32) {
        pixel.location.write(self.memory, value);
    }

    pub fn text_cell(&self, row: u16, column: u16) -> Option<TextCell> {
        let Scanout::Vga(state) = &self.scanout else {
            return None;
        };
        let address = Addressing::new(state)?.text_cell(row, column)?;
        Some(TextCell {
            address,
            value: u16::from_le_bytes([
                self.memory.read_physical(address),
                self.memory.read_physical(address + 1),
            ]),
        })
    }

    pub fn write_text_cell(&mut self, cell: &TextCell, value: u16) {
        for (offset, byte) in value.to_le_bytes().into_iter().enumerate() {
            self.memory
                .write_physical(cell.address + offset as u32, byte);
        }
    }
}
