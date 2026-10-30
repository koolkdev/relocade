use super::{access::PlanarAccess, Mode};
use relocade_machine::{IoBus, MemoryView};

pub(in crate::video) enum Pixel {
    Indexed { address: u32 },
    Planar { address: u32, mask: u8 },
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
        let address = page.address(x, y);
        Some(if page.mode.planar {
            Self::Planar {
                address,
                mask: 0x80 >> (x % 8),
            }
        } else {
            Self::Indexed { address }
        })
    }

    pub fn read(self, memory: &MemoryView<'_>, io: &IoBus) -> u8 {
        match self {
            Self::Indexed { address } => memory.read_u8(address).expect("graphics pixel"),
            Self::Planar { address, mask } => {
                PlanarAccess::new(io).read_pixel(memory, address, mask)
            }
        }
    }

    pub fn write(self, memory: &mut MemoryView<'_>, io: &IoBus, color: u8) {
        match self {
            // In 256-color modes bit seven is part of the color, not XOR.
            Self::Indexed { address } => memory.write_u8(address, color).expect("graphics pixel"),
            Self::Planar { address, mask } => {
                PlanarAccess::new(io).write_pixel(memory, address, mask, color)
            }
        }
    }
}
