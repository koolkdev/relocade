//! ET4000's VBE services; capabilities come from this board's indexed modes.
mod modes;
use super::modes::Mode;
use crate::video::firmware::vbe::{self, WindowAccess};
use relocade_machine::{
    real_mode_interrupt::{interrupt, interrupt_dispatch},
    CpuContext, FarPointer, MemoryView,
};
use relocade_vga::Et4000Ax;
use std::cell::RefCell;

#[interrupt(context(call), args(function = al))]
pub(in crate::video::firmware) async fn dispatch(
    call: &mut CpuContext<'_>,
    card: &RefCell<Et4000Ax>,
    function: u8,
) {
    interrupt_dispatch!(
        call,
        match function {
            0 => controller(card),
            1 => mode(card),
            2 => modes::set(card),
            3 => modes::current(card),
            5 => modes::window(card),
            _ => unsupported,
        }
    );
}

#[interrupt(returns { () => { ax = 0x0100 } })]
fn unsupported() {}

#[interrupt(context(memory), args(destination = ptr(es, di)), returns { true => { ax = 0x004f }, false => { ax = 0x014f } })]
fn controller(
    memory: &mut MemoryView<'_>,
    card: &RefCell<Et4000Ax>,
    destination: FarPointer,
) -> bool {
    let memory_bytes = {
        let card = card.borrow();
        if !card.ports_enabled() {
            return false;
        }
        card.video_memory_bytes()
    };
    vbe::controller(memory, destination, memory_bytes);
    true
}

#[interrupt(context(memory), args(number = cx, destination = ptr(es, di)), returns { true => { ax = 0x004f }, false => { ax = 0x014f } })]
fn mode(
    memory: &mut MemoryView<'_>,
    card: &RefCell<Et4000Ax>,
    number: u16,
    destination: FarPointer,
) -> bool {
    let layout = {
        let card = card.borrow();
        let Some(mode) = Mode::from_vbe_number(number).filter(|_| card.ports_enabled()) else {
            return false;
        };
        let Some(layout) = mode.layout(card.video_memory_bytes()) else {
            return false;
        };
        card.packed_layout()
            .filter(|active| active.size() == layout.size())
            .unwrap_or(layout)
    };
    // Window A controls writes, B controls reads; both use the A000h aperture.
    vbe::banked_mode(
        memory,
        destination,
        layout,
        [WindowAccess::Write, WindowAccess::Read],
    );
    true
}
