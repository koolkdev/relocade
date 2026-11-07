//! BIOS selection of sixteen-color or sixty-four-color palette pages.
use super::{read_attribute, write_raw_attribute};
use crate::video::{modes, MODE};
use relocade_machine::{real_mode_interrupt::interrupt, IoBus, MemoryView};
use relocade_vga::registers::attribute;

pub(in crate::video) struct Paging {
    pub mode: u8,
    pub page: u8,
}

#[interrupt(context(memory, io), args(mode = bh),
    returns { true => {}, false => crate::interrupts::unsupported },
)]
pub(in crate::video) fn set_mode(memory: &MemoryView<'_>, io: &IoBus, mode: u8) -> bool {
    if mode > 1 || memory.read_u8(MODE).expect("video mode") == modes::GRAPHICS_320X200_256 {
        return false;
    }
    let previous = read_attribute(io, attribute::MODE_CONTROL);
    write_raw_attribute(
        io,
        attribute::MODE_CONTROL,
        (previous & !0x80) | (mode << 7),
    );
    true
}

#[interrupt(context(memory, io), args(page = bh),
    returns { true => {}, false => crate::interrupts::unsupported },
)]
pub(in crate::video) fn select_page(memory: &MemoryView<'_>, io: &IoBus, page: u8) -> bool {
    if memory.read_u8(MODE).expect("video mode") == modes::GRAPHICS_320X200_256 {
        return false;
    }
    let mode = read_attribute(io, attribute::MODE_CONTROL);
    let bits = if mode & 0x80 != 0 {
        page & 15
    } else {
        (page & 3) << 2
    };
    write_raw_attribute(io, attribute::COLOR_SELECT, bits);
    true
}

#[interrupt(context(io), returns { paging => { bl = paging.mode, bh = paging.page } })]
pub(in crate::video) fn paging(io: &IoBus) -> Paging {
    let mode = read_attribute(io, attribute::MODE_CONTROL) >> 7;
    let bits = read_attribute(io, attribute::COLOR_SELECT);
    Paging {
        mode,
        page: if mode == 0 { bits >> 2 } else { bits },
    }
}
