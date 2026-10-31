//! INT 10h services. BIOS bookkeeping never selects rendering.

mod fonts;
pub(super) mod graphics;
mod modes;
mod palette;
mod presets;

pub(super) use fonts::Glyph;
pub(super) use modes::{set_mode, COLOR_TEXT_80X25};

use crate::screen::{self, ScrollDirection, Window};
use relocade_machine::{real_mode_interrupt, CpuContext, HookResult, IoBus, MemoryView};
use relocade_vga::registers::{CRTC_INDEX_PORT, MISC_OUTPUT_READ_PORT, MONO_CRTC_INDEX_PORT};

// Firmware video data in segment 0040h.
pub(super) const MODE: u32 = 0x449;
pub(super) const COLUMNS: u32 = 0x44a;
pub(super) const PAGE_SIZE: u32 = 0x44c;
pub(super) const PAGE_OFFSET: u32 = 0x44e;
pub(super) const CURSORS: u32 = 0x450;
pub(super) const CURSOR_SHAPE: u32 = 0x460;
pub(super) const ACTIVE_PAGE: u32 = 0x462;
const CRTC_PORT: u32 = 0x463;
pub(super) const LAST_ROW: u32 = 0x484;
pub(super) const CHARACTER_HEIGHT: u32 = 0x485;
const VIDEO_CONTROL: u32 = 0x487;
const VIDEO_SWITCHES: u32 = 0x488;
const MODESET_CONTROL: u32 = 0x489;

pub(super) fn initialize(memory: &mut MemoryView<'_>, io: &IoBus) {
    fonts::initialize(memory);
    modes::select_scanlines(memory, 0); // Preserve the existing doubled 8-high startup.
    set_mode(memory, io, COLOR_TEXT_80X25).expect("initial text mode");
}

pub(super) async fn interrupt(call: &mut CpuContext<'_>) -> HookResult {
    let cpu = &call.cpu;
    let function = cpu.ah();
    let al = cpu.al();
    let page = cpu.bh();
    let attribute = cpu.bl();
    let cx = cpu.cx();
    let dx = cpu.dx();
    match function {
        0x00 => {
            let Some(result) = set_mode(&mut call.memory, call.io, al) else {
                return unsupported(call);
            };
            call.cpu.set_al(result);
        }
        0x0f => {
            let memory = &call.memory;
            let mode = memory.read_u8(MODE).expect("video mode");
            let preserved = memory.read_u8(VIDEO_CONTROL).expect("video control") & 0x80;
            let columns = memory.read_u16(COLUMNS).expect("video columns") as u8;
            let active = screen::active_page(memory);
            call.cpu
                .set_ax(u16::from_le_bytes([mode | preserved, columns]));
            call.cpu.set_bh(active);
        }
        0x0c | 0x0d => {
            let Some(pixel) = graphics::Pixel::locate(&call.memory, page, cx, dx) else {
                return unsupported(call);
            };
            if function == 0x0c {
                pixel.write(&mut call.memory, call.io, al);
            } else {
                call.cpu.set_al(pixel.read(&call.memory, call.io));
            }
        }
        0x0b => return palette::compatibility(call),
        0x10 => return palette::interrupt(call),
        0x11 => return fonts::interrupt(call),
        0x12 if attribute == 0x30 && modes::select_scanlines(&mut call.memory, al) => {
            call.cpu.set_al(0x12);
        }
        0x02 | 0x03 if !screen::valid_page(&call.memory, page) => return unsupported(call),
        0x02 => screen::set_cursor_position(&mut call.memory, call.io, page, dx),
        0x03 => {
            let position = screen::cursor_position(&call.memory, page);
            let shape = call
                .memory
                .read_u16(CURSOR_SHAPE)
                .expect("video cursor shape");
            call.cpu.set_dx(position);
            call.cpu.set_cx(shape);
        }
        0x05 if screen::valid_page(&call.memory, al) => {
            screen::select_page(&mut call.memory, call.io, al)
        }
        0x06 | 0x07 => {
            let [left, top] = cx.to_le_bytes();
            let [right, bottom] = dx.to_le_bytes();
            let memory = &mut call.memory;
            screen::scroll(
                memory,
                call.io,
                Window {
                    top,
                    left,
                    bottom,
                    right,
                },
                al,
                page, // BH supplies the text attribute or graphics fill color.
                if function == 0x06 {
                    ScrollDirection::Up
                } else {
                    ScrollDirection::Down
                },
            );
        }
        0x01 if screen::is_text_mode(&call.memory) => {
            screen::set_cursor_shape(&mut call.memory, call.io, cx)
        }
        0x08 if screen::is_text_mode(&call.memory) && screen::valid_page(&call.memory, page) => {
            let cell = screen::read_cell(&call.memory, page);
            call.cpu.set_ax(u16::from_le_bytes(cell));
        }
        0x09 | 0x0a if screen::valid_page(&call.memory, page) => screen::write_repeated(
            &mut call.memory,
            call.io,
            page,
            al,
            attribute,
            cx,
            function == 0x09,
        ),
        0x0e if screen::valid_page(&call.memory, screen::active_page(&call.memory)) => {
            screen::teletype(&mut call.memory, call.io, al, attribute)
        }
        _ => return unsupported(call),
    }
    real_mode_interrupt::return_from_interrupt(call)
}

fn write_indexed(io: &IoBus, port: u16, register: u8, value: u8) {
    io.write_port(port, 2, u32::from(u16::from_le_bytes([register, value])));
}

fn read_indexed(io: &IoBus, port: u16, register: u8) -> u8 {
    io.write_port(port, 1, u32::from(register));
    io.read_port(port + 1, 1) as u8
}

// BIOS transfer buffers advance a 16-bit ES offset, including within a glyph
// or RGB triple. The next byte after ES:FFFF is ES:0000.
fn buffer_addresses(base: u32, offset: u16, bytes: u32) -> impl Iterator<Item = u32> {
    (0..bytes).map(move |index| base + u32::from(offset.wrapping_add(index as u16)))
}

pub(super) fn write_crtc_word(io: &IoBus, register: u8, value: u16) {
    let [high, low] = value.to_be_bytes();
    let port = crtc_port(io);
    write_indexed(io, port, register, high);
    write_indexed(io, port, register + 1, low);
}

fn crtc_port(io: &IoBus) -> u16 {
    if io.read_port(MISC_OUTPUT_READ_PORT, 1) & 1 != 0 {
        CRTC_INDEX_PORT
    } else {
        MONO_CRTC_INDEX_PORT
    }
}

fn unsupported(call: &mut CpuContext<'_>) -> HookResult {
    call.cpu.set_ah(0x86);
    real_mode_interrupt::set_carry(call, true);
    real_mode_interrupt::return_from_interrupt(call)
}
