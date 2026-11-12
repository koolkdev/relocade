//! VBE 4F04h: complete programming state, excluding framebuffer memory.
use super::available;
use crate::video::state::firmware;
use relocade_machine::{
    real_mode_interrupt::{interrupt, interrupt_dispatch},
    CpuContext, FarPointer, IoBus, MemoryView,
};
use relocade_vga::{DacState, DispiAdapter, DispiState, VgaCoreState};
use std::cell::RefCell;

const HEADER_SIZE: usize = 16;
const COMPONENT_SIZES: [usize; 4] = [
    VgaCoreState::BYTE_LEN,
    firmware::SIZE,
    DacState::BYTE_LEN,
    DispiState::BYTE_LEN,
];

/// Relocade's opaque, versioned buffer. Relative offsets allow guest relocation.
struct BufferLayout {
    components: u16,
    offsets: [u16; 4],
    bytes: u16,
}

impl BufferLayout {
    fn new(components: u16) -> Option<Self> {
        if components == 0 || components & !0x0f != 0 {
            return None;
        }
        let mut offsets = [0; 4];
        let mut bytes = HEADER_SIZE as u16;
        for (index, size) in COMPONENT_SIZES.into_iter().enumerate() {
            if components & (1 << index) != 0 {
                offsets[index] = bytes;
                bytes += size as u16;
            }
        }
        Some(Self {
            components,
            offsets,
            bytes,
        })
    }

    fn header(&self) -> [u8; HEADER_SIZE] {
        let mut header = [0; HEADER_SIZE];
        header[..6].copy_from_slice(b"RVBE\x01\x00");
        header[6..8].copy_from_slice(&self.components.to_le_bytes());
        for (index, offset) in self.offsets.into_iter().enumerate() {
            header[8 + index * 2..10 + index * 2].copy_from_slice(&offset.to_le_bytes());
        }
        header
    }

    fn from_header(header: &[u8; HEADER_SIZE]) -> Option<Self> {
        let components = u16::from_le_bytes(header[6..8].try_into().unwrap());
        let layout = Self::new(components)?;
        (layout.header() == *header).then_some(layout)
    }

    fn component(&self, pointer: FarPointer, index: usize) -> FarPointer {
        pointer.wrapping_add(usize::from(self.offsets[index]))
    }
}

#[interrupt(context(call), args(operation = dl))]
pub(super) async fn dispatch(
    call: &mut CpuContext<'_>,
    vga: &RefCell<DispiAdapter>,
    operation: u8,
) {
    interrupt_dispatch!(
        call,
        match operation {
            0 => required_blocks,
            1 => save(vga),
            2 => restore(vga),
            _ => failed,
        }
    );
}

#[interrupt(context(io), args(components = cx), returns {
    Some(blocks) => { ax = 0x004f, bx = blocks }, None => { ax = 0x014f },
})]
fn required_blocks(io: &IoBus, components: u16) -> Option<u16> {
    available(io).then_some(())?;
    Some(BufferLayout::new(components)?.bytes.div_ceil(64))
}

#[interrupt(context(memory, io), args(components = cx, destination = ptr(es, bx)),
    returns { Some(()) => { ax = 0x004f }, None => { ax = 0x014f } },
)]
fn save(
    memory: &mut MemoryView<'_>,
    io: &IoBus,
    vga: &RefCell<DispiAdapter>,
    components: u16,
    destination: FarPointer,
) -> Option<()> {
    available(io).then_some(())?;
    let layout = BufferLayout::new(components)?;
    // Capture everything before writing: the buffer may overlap BIOS data or VRAM.
    let hardware = (components & 1 != 0).then(|| vga.borrow().core_state().to_bytes());
    let firmware = (components & 2 != 0).then(|| firmware::save(memory));
    let dac = (components & 4 != 0).then(|| vga.borrow().dac_state().to_bytes());
    let dispi = (components & 8 != 0).then(|| vga.borrow().dispi_state().to_bytes());
    let blocks = [
        hardware.as_ref().map(|bytes| bytes.as_slice()),
        firmware.as_ref().map(|bytes| bytes.as_slice()),
        dac.as_ref().map(|bytes| bytes.as_slice()),
        dispi.as_ref().map(|bytes| bytes.as_slice()),
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
fn restore(
    memory: &mut MemoryView<'_>,
    io: &IoBus,
    vga: &RefCell<DispiAdapter>,
    components: u16,
    source: FarPointer,
) -> Option<()> {
    available(io).then_some(())?;
    let layout = BufferLayout::from_header(&read_buffer(memory, source))?;
    if components == 0 || components & !layout.components != 0 {
        return None;
    }
    // Validate all requested device classes before changing any state. Read BIOS
    // data before remapping VRAM or writing potentially overlapping destinations.
    let hardware = if components & 1 != 0 {
        Some(VgaCoreState::from_bytes(&read_buffer(
            memory,
            layout.component(source, 0),
        ))?)
    } else {
        None
    };
    let firmware = (components & 2 != 0).then(|| read_buffer(memory, layout.component(source, 1)));
    let dac = if components & 4 != 0 {
        Some(DacState::from_bytes(&read_buffer(
            memory,
            layout.component(source, 2),
        ))?)
    } else {
        None
    };
    let dispi = if components & 8 != 0 {
        Some(DispiState::from_bytes(&read_buffer(
            memory,
            layout.component(source, 3),
        ))?)
    } else {
        None
    };
    io.with_context(|context| {
        vga.borrow_mut()
            .restore_state(context, hardware, dac, dispi)
    });
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
