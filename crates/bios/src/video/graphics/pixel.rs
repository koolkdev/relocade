use super::{access::GraphicsAccess, Layout, Mode};
use relocade_machine::{IoBus, MemoryView};

pub(in crate::video) struct Pixel {
    address: u32,
    layout: Layout,
    shift: u8,
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
