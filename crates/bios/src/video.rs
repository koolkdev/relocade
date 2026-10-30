//! INT 10h services for the initial 80-by-25 color text mode.

use crate::screen::{self, ScrollDirection, Window};
use relocade_machine::{real_mode_interrupt, CallContext, HookResult};

pub(super) async fn interrupt<T>(mut call: CallContext<T>) -> HookResult {
    let cpu = &call.cpu;
    let function = cpu.ah();
    let al = cpu.al();
    let page = cpu.bh();
    let attribute = cpu.bl();
    let cx = cpu.cx();
    let dx = cpu.dx();
    if matches!(function, 0x02 | 0x03 | 0x08..=0x0a) && !screen::valid_page(page) {
        return unsupported(&mut call);
    }
    match function {
        0x00 if al == 3 => {
            screen::initialize(&mut call.memory.view(), &call.io);
            call.cpu.set_al(0x30); // BIOS mode-set return for color text modes.
        }
        0x01 => screen::set_cursor_shape(&mut call.memory.view(), &call.io, cx),
        0x02 => screen::set_cursor_position(&mut call.memory.view(), &call.io, page, dx),
        0x03 => {
            let position = screen::cursor_position(&call.memory.view(), page);
            let shape = call
                .memory
                .view()
                .read_u16(screen::CURSOR_SHAPE)
                .expect("video cursor shape");
            call.cpu.set_dx(position);
            call.cpu.set_cx(shape);
        }
        0x05 if screen::valid_page(al) => {
            screen::select_page(&mut call.memory.view(), &call.io, al)
        }
        0x06 | 0x07 => {
            let [left, top] = cx.to_le_bytes();
            let [right, bottom] = dx.to_le_bytes();
            let memory = &mut call.memory.view();
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
            let cell = screen::read_cell(&call.memory.view(), page);
            call.cpu.set_ax(u16::from_le_bytes(cell));
        }
        0x09 | 0x0a => screen::write_repeated(
            &mut call.memory.view(),
            page,
            al,
            (function == 0x09).then_some(attribute),
            cx,
        ),
        0x0e => screen::write_text(&mut call.memory.view(), &call.io, &[al]),
        0x0f => {
            let memory = &call.memory.view();
            let mode = memory.read_u8(screen::MODE).expect("video mode");
            let columns = memory.read_u16(screen::COLUMNS).expect("video columns") as u8;
            let active = screen::active_page(memory);
            call.cpu.set_ax(u16::from_le_bytes([mode, columns]));
            call.cpu.set_bh(active);
        }
        _ => return unsupported(&mut call),
    }
    real_mode_interrupt::return_from_interrupt(&mut call.cpu)
}

fn unsupported<T>(call: &mut CallContext<T>) -> HookResult {
    call.cpu.set_ah(0x86);
    real_mode_interrupt::set_carry(&call.cpu, &mut call.memory.view(), true);
    real_mode_interrupt::return_from_interrupt(&mut call.cpu)
}
