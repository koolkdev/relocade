//! Physical pixel fields shared by sampling, replacement and restoration.
use crate::{memory::VideoMemory, PixelFormat};

#[derive(Clone, Copy)]
pub(super) enum Location {
    Indexed {
        address: u32,
        rotation: u8,
    },
    Planar {
        address: u32,
        shift: u8,
        interleaved: bool,
        rotation: u8,
    },
    Packed {
        address: u32,
        format: PixelFormat,
    },
}

fn rotate(color: u8, shift: u8) -> u8 {
    let color = color & 15;
    ((color >> shift) | (color << (4 - shift))) & 15
}

impl Location {
    pub fn read(self, memory: &VideoMemory) -> u32 {
        u32::from(match self {
            Self::Indexed { address, rotation } => {
                let color = memory.read_physical(address);
                (rotate(color >> 4, rotation) << 4) | rotate(color & 15, rotation)
            }
            Self::Planar {
                address,
                shift,
                interleaved,
                rotation,
            } => {
                let color = if interleaved {
                    ((memory.read_physical(address) >> shift) & 3)
                        | (((memory.read_physical(address + 2) >> shift) & 3) << 2)
                } else {
                    (0..4).fold(0, |color, plane| {
                        color | (((memory.read_physical(address + plane) >> shift) & 1) << plane)
                    })
                };
                rotate(color, rotation)
            }
            Self::Packed { address, format } => {
                let mut bytes = [0; 4];
                for (offset, byte) in bytes[..format.bytes_per_pixel() as usize]
                    .iter_mut()
                    .enumerate()
                {
                    *byte = memory.read_physical(address + offset as u32);
                }
                return u32::from_le_bytes(bytes);
            }
        })
    }

    pub fn write(self, memory: &mut VideoMemory, value: u32) {
        match self {
            Self::Indexed { address, rotation } => {
                let color = value as u8;
                memory.write_physical(
                    address,
                    (rotate(color >> 4, (4 - rotation) & 3) << 4)
                        | rotate(color & 15, (4 - rotation) & 3),
                );
            }
            Self::Planar {
                address,
                shift,
                interleaved,
                rotation,
            } => {
                let color = rotate(value as u8, (4 - rotation) & 3);
                let (planes, bits, step) = if interleaved { (2, 2, 2) } else { (4, 1, 1) };
                let mask = (1 << bits) - 1;
                for plane in 0..planes {
                    let address = address + plane * step;
                    let previous = memory.read_physical(address);
                    memory.write_physical(
                        address,
                        (previous & !(mask << shift))
                            | (((color >> (plane * bits)) & mask) << shift),
                    );
                }
            }
            Self::Packed { address, format } => {
                for (offset, byte) in value.to_le_bytes()[..format.bytes_per_pixel() as usize]
                    .iter()
                    .enumerate()
                {
                    memory.write_physical(address + offset as u32, *byte);
                }
            }
        }
    }
}
