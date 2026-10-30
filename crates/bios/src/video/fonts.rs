//! INT 10h/AH=11h loads fonts through VGA memory and programs text geometry.
mod glyph;
mod layout;
mod rom;
mod upload;

use super::{buffer_addresses, unsupported, write_indexed, CHARACTER_HEIGHT, LAST_ROW};
use crate::screen;
pub(crate) use glyph::Glyph;
use relocade_machine::{real_mode_interrupt, CpuContext, HookResult, IoBus, MemoryView};
use relocade_vga::registers::{sequencer, SEQUENCER_INDEX_PORT};
use relocade_x86::{Segment, SegmentRegister};
pub(super) use rom::initialize;

pub(super) fn interrupt<T>(call: &mut CpuContext<'_, T>) -> HookResult {
    let function = call.cpu.al();
    if function == 0x30 {
        return information(call);
    }
    if !screen::is_text_mode(&call.memory) {
        return unsupported(call);
    }
    if function == 0x03 {
        write_indexed(
            call.io,
            SEQUENCER_INDEX_PORT,
            sequencer::CHARACTER_MAP_SELECT,
            call.cpu.bl(),
        );
        return real_mode_interrupt::return_from_interrupt(call);
    }
    let builtin = match function {
        0x00 | 0x10 => None,
        0x01 | 0x11 => Some(rom::for_height(14)),
        0x02 | 0x12 => Some(rom::for_height(8)),
        0x04 | 0x14 => Some(rom::for_height(16)),
        _ => return unsupported(call),
    };
    let height = builtin.map_or(call.cpu.bh(), |font| font.height);
    let bank = call.cpu.bl();
    let (first, count) = if builtin.is_some() {
        (0, 256)
    } else {
        (call.cpu.dx(), call.cpu.cx())
    };
    if !(1..=32).contains(&height) || bank >= 8 || u32::from(first) + u32::from(count) > 256 {
        return unsupported(call);
    }
    let layout = if function & 0x10 != 0 {
        let Some(layout) = layout::Layout::for_font(&call.memory, call.io, height) else {
            return unsupported(call);
        };
        Some(layout)
    } else {
        None
    };
    if count != 0 {
        // Read the source before exposing plane two: ES can itself address VGA.
        let guest_bytes;
        let bytes = if let Some(font) = builtin {
            font.bytes
        } else {
            guest_bytes = buffer_addresses(
                call.cpu.segments().es.base,
                call.cpu.bp(),
                u32::from(count) * u32::from(height),
            )
            .map(|address| call.memory.read_u8(address).expect("font buffer"))
            .collect::<Vec<_>>();
            &guest_bytes
        };
        upload::write_glyphs(
            &mut call.memory,
            call.io,
            bank,
            first,
            bytes.chunks(usize::from(height)),
        );
    }
    if let Some(layout) = layout {
        layout.apply(&mut call.memory, call.io);
    }
    real_mode_interrupt::return_from_interrupt(call)
}

fn information<T>(call: &mut CpuContext<'_, T>) -> HookResult {
    let Some(pointer) = rom::query(&call.memory, call.cpu.bh()) else {
        return unsupported(call);
    };
    let height = call.memory.read_u16(CHARACTER_HEIGHT).expect("font height");
    let last_row = call.memory.read_u8(LAST_ROW).expect("video rows");
    call.cpu.set_segment(
        Segment::Es,
        SegmentRegister::real_mode(Segment::Es, (pointer >> 16) as u16),
    );
    call.cpu.set_bp(pointer as u16);
    // CX describes the current display even when BH asks for another ROM font.
    call.cpu.set_cx(height);
    call.cpu.set_dl(last_row);
    real_mode_interrupt::return_from_interrupt(call)
}

pub(super) fn set_mode(memory: &mut MemoryView<'_>, io: &IoBus, height: u8, text: bool) {
    let font = rom::for_height(height);
    memory
        .write_u32(0x43 * 4, font.pointer())
        .expect("graphics font vector");
    if text {
        // Mode setting resets complete slots. AH=11h instead preserves all
        // bytes below each supplied glyph, even when loading a built-in font.
        let glyphs = font.bytes.chunks(usize::from(height)).map(|glyph| {
            let mut slot = [0; 32];
            slot[..glyph.len()].copy_from_slice(glyph);
            slot
        });
        upload::write_glyphs(memory, io, 0, 0, glyphs);
    }
}
