//! BIOS pixel access through the current graphics layout.

use super::{access::GraphicsAccess, Layout, Mode};
use relocade_machine::{real_mode_interrupt::interrupt, IoBus, MemoryView};

struct Pixel {
    address: u32,
    layout: Layout,
    shift: u8,
}

/// Reads a graphics pixel without changing guest VGA access settings.
#[interrupt(context(memory, io), args(page = bh, x = cx, y = dx), returns {
    Some(color) => { al = color },
    None => crate::interrupt::unsupported,
})]
pub fn read_pixel(memory: &MemoryView<'_>, io: &IoBus, page: u8, x: u16, y: u16) -> Option<u8> {
    Some(Pixel::locate(memory, page, x, y)?.read(memory, io))
}

/// Writes a graphics pixel, including the mode's existing XOR/color rules.
#[interrupt(context(memory, io), args(page = bh, x = cx, y = dx, color = al),
    returns { Some(()) => {}, None => crate::interrupt::unsupported }
)]
pub fn write_pixel(
    memory: &mut MemoryView<'_>,
    io: &IoBus,
    page: u8,
    x: u16,
    y: u16,
    color: u8,
) -> Option<()> {
    Pixel::locate(memory, page, x, y)?.write(memory, io, color);
    Some(())
}

impl Pixel {
    /// Pixel services ignore BH in single-page modes; page selection does not.
    pub fn locate(memory: &MemoryView<'_>, page: u8, x: u16, y: u16) -> Option<Self> {
        let mode = Mode::read(memory)?;
        if x >= mode.width || y >= mode.height {
            return None;
        }
        let page_number = if mode.pages == 1 { 0 } else { page };
        let page = mode.page(page_number)?;
        Some(Self {
            address: page.address(x, y),
            layout: page.mode.layout,
            shift: page.mode.layout.shift(x),
        })
    }

    pub fn read(self, memory: &MemoryView<'_>, io: &IoBus) -> u8 {
        let access = GraphicsAccess::new(io, self.layout.write_planes());
        if self.layout == Layout::Planar {
            access.read_planar_pixel(memory, self.address, 1 << self.shift)
        } else {
            (memory.read_u8(self.address).expect("graphics pixel") >> self.shift)
                & self.layout.pixel_mask()
        }
    }

    pub fn write(self, memory: &mut MemoryView<'_>, io: &IoBus, color: u8) {
        let access = GraphicsAccess::new(io, self.layout.write_planes());
        match self.layout {
            Layout::Planar => {
                access.write_planar_masked(memory, self.address, 1 << self.shift, color)
            }
            // In 256-color modes bit seven is part of the color, not XOR.
            Layout::Indexed256 => memory
                .write_u8(self.address, color)
                .expect("graphics pixel"),
            Layout::Packed1 | Layout::Packed2 => {
                let previous = memory.read_u8(self.address).expect("packed pixel byte");
                let mask = self.layout.pixel_mask() << self.shift;
                let value = (color & self.layout.pixel_mask()) << self.shift;
                let byte = if color & 0x80 != 0 {
                    previous ^ value
                } else {
                    (previous & !mask) | value
                };
                memory
                    .write_u8(self.address, byte)
                    .expect("packed pixel byte");
            }
        }
    }
}
