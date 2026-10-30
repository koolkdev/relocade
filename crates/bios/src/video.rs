//! INT 10h mode setup and text services. BIOS bookkeeping never selects rendering.

mod modes;
mod palette;
mod presets;

pub(super) use modes::set_mode;

use crate::screen::{self, ScrollDirection, Window};
use relocade_machine::{real_mode_interrupt, CpuContext, HookResult, IoBus};
use relocade_vga::registers::CRTC_INDEX_PORT;

// Firmware video data in segment 0040h.
pub(super) const MODE: u32 = 0x449;
pub(super) const COLUMNS: u32 = 0x44a;
const PAGE_SIZE: u32 = 0x44c;
pub(super) const PAGE_OFFSET: u32 = 0x44e;
pub(super) const CURSORS: u32 = 0x450;
pub(super) const CURSOR_SHAPE: u32 = 0x460;
pub(super) const ACTIVE_PAGE: u32 = 0x462;
const CRTC_PORT: u32 = 0x463;
const LAST_ROW: u32 = 0x484;
const VIDEO_CONTROL: u32 = 0x487;

pub(super) async fn interrupt<T>(call: &mut CpuContext<'_, T>) -> HookResult {
    let cpu = &call.cpu;
    let function = cpu.ah();
    let al = cpu.al();
    let page = cpu.bh();
    let attribute = cpu.bl();
    let cx = cpu.cx();
    let dx = cpu.dx();
    if !matches!(function, 0x00 | 0x0f) && !screen::is_text_mode(&call.memory) {
        return unsupported(call);
    }
    if matches!(function, 0x02 | 0x03 | 0x08..=0x0a) && !screen::valid_page(page) {
        return unsupported(call);
    }
    match function {
        0x00 => {
            let Some(result) = set_mode(&mut call.memory, call.io, al) else {
                return unsupported(call);
            };
            call.cpu.set_al(result);
        }
        0x01 => screen::set_cursor_shape(&mut call.memory, call.io, cx),
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
        0x05 if screen::valid_page(al) => screen::select_page(&mut call.memory, call.io, al),
        0x06 | 0x07 => {
            let [left, top] = cx.to_le_bytes();
            let [right, bottom] = dx.to_le_bytes();
            let memory = &mut call.memory;
            let active = screen::active_page(memory);
            screen::scroll(
                memory,
                active,
                Window {
                    top,
                    left,
                    bottom,
                    right,
                },
                al,
                page, // BH is the blank-cell attribute for scrolling.
                if function == 0x06 {
                    ScrollDirection::Up
                } else {
                    ScrollDirection::Down
                },
            );
        }
        0x08 => {
            let cell = screen::read_cell(&call.memory, page);
            call.cpu.set_ax(u16::from_le_bytes(cell));
        }
        0x09 | 0x0a => screen::write_repeated(
            &mut call.memory,
            page,
            al,
            (function == 0x09).then_some(attribute),
            cx,
        ),
        0x0e => screen::write_text(&mut call.memory, call.io, &[al]),
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
        _ => return unsupported(call),
    }
    real_mode_interrupt::return_from_interrupt(call)
}

fn write_indexed(io: &IoBus, port: u16, register: u8, value: u8) {
    io.write_port(port, 2, u32::from(u16::from_le_bytes([register, value])));
}

pub(super) fn write_crtc_word(io: &IoBus, register: u8, value: u16) {
    let [high, low] = value.to_be_bytes();
    write_indexed(io, CRTC_INDEX_PORT, register, high);
    write_indexed(io, CRTC_INDEX_PORT, register + 1, low);
}

fn unsupported<T>(call: &mut CpuContext<'_, T>) -> HookResult {
    call.cpu.set_ah(0x86);
    real_mode_interrupt::set_carry(call, true);
    real_mode_interrupt::return_from_interrupt(call)
}
