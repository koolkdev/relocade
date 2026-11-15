//! VBE 4F04h: complete programming state, excluding framebuffer memory.
use super::Vbe;
use crate::video::state::firmware;
use relocade_machine::{
    real_mode_interrupt::{interrupt, interrupt_dispatch},
    CpuContext, FarPointer, IoBus, MemoryView,
};
use relocade_vga::{DacState, VgaCoreState};
use std::cell::RefCell;

const HEADER_SIZE: usize = 16;
/// Relocade's opaque, versioned buffer. Relative offsets allow guest relocation.
struct BufferLayout {
    profile: u8,
    components: u16,
    offsets: [u16; 4],
    bytes: u16,
}

impl BufferLayout {
    fn new<C: Vbe>(components: u16) -> Option<Self> {
        if components == 0 || components & !0x0f != 0 {
            return None;
        }
        let mut offsets = [0; 4];
        let mut bytes = HEADER_SIZE as u16;
        for (index, size) in [
            VgaCoreState::BYTE_LEN,
            firmware::SIZE,
            DacState::BYTE_LEN,
            C::EXTENSION_STATE_BYTES,
        ]
        .into_iter()
        .enumerate()
        {
            if components & (1 << index) != 0 {
                offsets[index] = bytes;
                bytes += size as u16;
            }
        }
        Some(Self {
            profile: C::STATE_PROFILE,
            components,
            offsets,
            bytes,
        })
    }

    fn header(&self) -> [u8; HEADER_SIZE] {
        let mut header = [0; HEADER_SIZE];
        header[..5].copy_from_slice(b"RVBE\x01");
        header[5] = self.profile;
        header[6..8].copy_from_slice(&self.components.to_le_bytes());
        for (index, offset) in self.offsets.into_iter().enumerate() {
            header[8 + index * 2..10 + index * 2].copy_from_slice(&offset.to_le_bytes());
        }
        header
    }

    fn from_header<C: Vbe>(header: &[u8; HEADER_SIZE]) -> Option<Self> {
        let components = u16::from_le_bytes(header[6..8].try_into().unwrap());
        let layout = Self::new::<C>(components)?;
        (layout.header() == *header).then_some(layout)
    }

    fn component(&self, pointer: FarPointer, index: usize) -> FarPointer {
        pointer.wrapping_add(usize::from(self.offsets[index]))
    }
}

#[interrupt(context(call), args(operation = dl))]
pub(in crate::video::firmware) async fn dispatch<C: Vbe>(
    call: &mut CpuContext<'_>,
    card: &RefCell<C>,
    operation: u8,
) {
    interrupt_dispatch!(
        call,
        match operation {
            0 => required_blocks(card),
            1 => save(card),
            2 => restore(card),
            _ => failed,
        }
    );
}

#[interrupt(args(components = cx), returns {
    Some(blocks) => { ax = 0x004f, bx = blocks }, None => { ax = 0x014f },
})]
fn required_blocks<C: Vbe>(card: &RefCell<C>, components: u16) -> Option<u16> {
    card.borrow().ports_enabled().then_some(())?;
    Some(BufferLayout::new::<C>(components)?.bytes.div_ceil(64))
}

#[interrupt(context(memory), args(components = cx, destination = ptr(es, bx)),
    returns { Some(()) => { ax = 0x004f }, None => { ax = 0x014f } },
)]
fn save<C: Vbe>(
    memory: &mut MemoryView<'_>,
    card: &RefCell<C>,
    components: u16,
    destination: FarPointer,
) -> Option<()> {
    card.borrow().ports_enabled().then_some(())?;
    let layout = BufferLayout::new::<C>(components)?;
    // Capture everything before writing: the buffer may overlap BIOS data or VRAM.
    let state = card.borrow().capture_state();
    let firmware = (components & 2 != 0).then(|| firmware::save(memory));
    let blocks = [
        (components & 1 != 0).then_some(state.core.as_slice()),
        firmware.as_ref().map(|bytes| bytes.as_slice()),
        (components & 4 != 0).then_some(state.dac.as_slice()),
        (components & 8 != 0).then_some(state.extensions.as_slice()),
    ];
    memory
        .write_wrapping(destination, &layout.header())
        .expect("VBE state header");
    for (index, block) in blocks.into_iter().enumerate() {
        if let Some(bytes) = block {
            memory
                .write_wrapping(layout.component(destination, index), bytes)
                .expect("VBE state buffer");
        }
    }
    Some(())
}

#[interrupt(context(memory, io), args(components = cx, source = ptr(es, bx)),
    returns { Some(()) => { ax = 0x004f }, None => { ax = 0x014f } },
)]
fn restore<C: Vbe>(
    memory: &mut MemoryView<'_>,
    io: &IoBus,
    card: &RefCell<C>,
    components: u16,
    source: FarPointer,
) -> Option<()> {
    card.borrow().ports_enabled().then_some(())?;
    let layout = BufferLayout::from_header::<C>(&read_buffer(memory, source))?;
    if components == 0 || components & !layout.components != 0 {
        return None;
    }
    // Validate all requested device classes before changing any state. Read BIOS
    // data before remapping VRAM or writing potentially overlapping destinations.
    let hardware: Option<[u8; VgaCoreState::BYTE_LEN]> =
        (components & 1 != 0).then(|| read_buffer(memory, layout.component(source, 0)));
    let firmware = (components & 2 != 0).then(|| read_buffer(memory, layout.component(source, 1)));
    let dac: Option<[u8; DacState::BYTE_LEN]> =
        (components & 4 != 0).then(|| read_buffer(memory, layout.component(source, 2)));
    let extensions = (components & 8 != 0).then(|| {
        let mut bytes = vec![0; C::EXTENSION_STATE_BYTES];
        memory
            .read_wrapping(layout.component(source, 3), &mut bytes)
            .expect("VBE extension state");
        bytes
    });
    io.with_context(|context| {
        card.borrow_mut().restore_state(
            context,
            hardware.as_ref(),
            dac.as_ref(),
            extensions.as_deref(),
        )
    })?;
    if let Some(bytes) = firmware {
        firmware::restore(memory, &bytes);
    }
    Some(())
}

#[interrupt(returns { () => { ax = 0x014f } })]
fn failed() {}

fn read_buffer<const N: usize>(memory: &MemoryView<'_>, pointer: FarPointer) -> [u8; N] {
    let mut bytes = [0; N];
    memory
        .read_wrapping(pointer, &mut bytes)
        .expect("VBE state buffer");
    bytes
}
