//! VBE DAC format and palette transfers through the selected card's driver.
//! VBE 2.0 sections 4.11–4.12: https://www.phatcode.net/res/221/files/vbe20.pdf
use super::Vbe;
use relocade_machine::{
    real_mode_interrupt::{interrupt, interrupt_dispatch},
    CpuContext, FarPointer, IoBus, MemoryView,
};
use relocade_vga::PixelFormat;
use std::cell::RefCell;

#[cfg(test)]
mod tests;

#[derive(Debug, PartialEq, Eq)]
#[repr(u16)]
pub(in crate::video::firmware) enum Error {
    Failed = 0x014f,
    Unsupported = 0x024f,
    DirectColor = 0x034f,
}

#[interrupt(context(call), args(operation = bl))]
pub(in crate::video::firmware) async fn format<C: Vbe>(
    call: &mut CpuContext<'_>,
    card: &RefCell<C>,
    operation: u8,
) {
    interrupt_dispatch!(
        call,
        match operation {
            0 => set_format(card),
            1 => get_format(card),
            _ => invalid_format(card),
        }
    );
}

#[interrupt(context(io), args(bits = bh), returns {
    Ok(bits) => { ax = 0x004f, bh = bits },
    Err(error) => { ax = error as u16 },
})]
fn set_format<C: Vbe>(io: &IoBus, card: &RefCell<C>, bits: u8) -> Result<u8, Error> {
    check_mode(&*card.borrow())?;
    // VBE 2.0 section 4.11: choose the next lower supported precision.
    let bits = if bits >= 8 && C::MAX_DAC_BITS_PER_COMPONENT == 8 {
        8
    } else if bits >= 6 {
        6
    } else {
        return Err(Error::Failed);
    };
    io.with_context(|context| card.borrow_mut().set_dac_bits_per_component(context, bits));
    Ok(bits)
}

#[interrupt(returns {
    Ok(bits) => { ax = 0x004f, bh = bits },
    Err(error) => { ax = error as u16 },
})]
fn get_format<C: Vbe>(card: &RefCell<C>) -> Result<u8, Error> {
    let card = card.borrow();
    check_mode(&*card)?;
    Ok(card.dac_bits_per_component())
}

#[interrupt(returns { error => { ax = error as u16 } })]
fn invalid_format<C: Vbe>(card: &RefCell<C>) -> Error {
    check_mode(&*card.borrow()).err().unwrap_or(Error::Failed)
}

#[interrupt(context(call), args(operation = bl))]
pub(in crate::video::firmware) async fn data<C: Vbe>(
    call: &mut CpuContext<'_>,
    card: &RefCell<C>,
    operation: u8,
) {
    interrupt_dispatch!(
        call,
        match operation {
            0 => set(card),
            1 => get(card),
            2 | 3 => unsupported, // Neither card has a secondary palette.
            0x80 => set_at_retrace(card),
            _ => failed,
        }
    );
}

#[interrupt(context(memory, io), args(count = cx, first = dx, buffer = ptr(es, di)),
    returns { Ok(()) => { ax = 0x004f }, Err(error) => { ax = error as u16 } },
)]
fn set<C: Vbe>(
    memory: &mut MemoryView<'_>,
    io: &IoBus,
    card: &RefCell<C>,
    count: u16,
    first: u16,
    buffer: FarPointer,
) -> Result<(), Error> {
    let colors = read_colors(memory, card, count, first, buffer)?;
    if !colors.is_empty() {
        io.with_context(|context| card.borrow_mut().set_palette(context, first as u8, &colors));
    }
    Ok(())
}

#[interrupt(context(memory, io), args(count = cx, first = dx, buffer = ptr(es, di)),
    returns { Ok(()) => { ax = 0x004f }, Err(error) => { ax = error as u16 } },
)]
async fn set_at_retrace<C: Vbe>(
    memory: &mut MemoryView<'_>,
    io: &IoBus,
    card: &RefCell<C>,
    count: u16,
    first: u16,
    buffer: FarPointer,
) -> Result<(), Error> {
    let colors = read_colors(memory, card, count, first, buffer)?;
    if colors.is_empty() {
        return Ok(());
    }
    let mode = card.borrow().current_mode(memory);
    let bits = card.borrow().dac_bits_per_component();
    super::retrace::wait(card, |card| {
        check_mode(card).is_ok()
            && card.current_mode(memory) == mode
            && card.dac_bits_per_component() == bits
    })
    .await
    .ok_or(Error::Failed)?;
    io.with_context(|context| card.borrow_mut().set_palette(context, first as u8, &colors));
    Ok(())
}

#[interrupt(context(memory), args(count = cx, first = dx, buffer = ptr(es, di)),
    returns { Ok(()) => { ax = 0x004f }, Err(error) => { ax = error as u16 } },
)]
fn get<C: Vbe>(
    memory: &mut MemoryView<'_>,
    card: &RefCell<C>,
    count: u16,
    first: u16,
    buffer: FarPointer,
) -> Result<(), Error> {
    let entries: Vec<_> = {
        let card = card.borrow();
        check_transfer(&*card, count, first)?;
        card.palette()[usize::from(first)..usize::from(first + count)]
            .iter()
            .map(|&[red, green, blue]| [blue, green, red, 0])
            .collect()
    };
    if !entries.is_empty() {
        // Release the card before a guest write can reenter its MMIO callbacks.
        memory
            .write_wrapping(buffer, entries.as_flattened())
            .expect("VBE palette buffer");
    }
    Ok(())
}

#[interrupt(returns { () => { ax = Error::Unsupported as u16 } })]
fn unsupported() {}

#[interrupt(returns { () => { ax = Error::Failed as u16 } })]
fn failed() {}

fn read_colors<C: Vbe>(
    memory: &MemoryView<'_>,
    card: &RefCell<C>,
    count: u16,
    first: u16,
    buffer: FarPointer,
) -> Result<Vec<[u8; 3]>, Error> {
    check_transfer(&*card.borrow(), count, first)?;
    // Own the input before programming or suspending: it may be in VRAM,
    // and an interrupt handler may change it during a retrace wait.
    Ok((0..count)
        .map(|offset| {
            let mut entry = [0; 4];
            memory
                .read_wrapping(buffer.wrapping_add(usize::from(offset) * 4), &mut entry)
                .expect("VBE palette buffer");
            let [blue, green, red, _] = entry;
            [red, green, blue]
        })
        .collect())
}

fn check_transfer<C: Vbe>(card: &C, count: u16, first: u16) -> Result<(), Error> {
    check_mode(card)?;
    if first >= 256 || count > 256 - first {
        return Err(Error::Failed);
    }
    Ok(())
}

fn check_mode<C: Vbe>(card: &C) -> Result<(), Error> {
    if !card.ports_enabled() {
        return Err(Error::Failed);
    }
    // These cards expose fixed ramps in direct-color modes.
    if card
        .packed_layout()
        .is_some_and(|layout| layout.format() != PixelFormat::Indexed8)
    {
        return Err(Error::DirectColor);
    }
    Ok(())
}
