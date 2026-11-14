//! VBE 4F04h: complete programming state, excluding framebuffer memory.
use super::super::VideoFirmware;
use crate::video::state::firmware;
use relocade_machine::{
    real_mode_interrupt::{interrupt, interrupt_dispatch},
    CpuContext, FarPointer, IoBus, MemoryView,
};
use relocade_vga::{DacState, DispiState, Et4000Ax, Et4000State, VgaCoreState};

const HEADER_SIZE: usize = 16;
/// Byte five identifies both the card and its fixed board profile. Zero keeps
/// existing DISPI buffers compatible; ET4000 state belongs to the 1-MiB EV8913.
#[derive(Clone, Copy)]
#[repr(u8)]
enum Profile {
    Dispi = 0,
    Et4000AxEv8913 = 1,
}

impl Profile {
    fn of(card: &VideoFirmware) -> Option<Self> {
        match card {
            VideoFirmware::Dispi(_) => Some(Self::Dispi),
            VideoFirmware::Et4000Ax(_) => Some(Self::Et4000AxEv8913),
            VideoFirmware::Vga => None,
        }
    }

    fn component_sizes(self) -> [usize; 4] {
        [
            VgaCoreState::BYTE_LEN,
            firmware::SIZE,
            DacState::BYTE_LEN,
            match self {
                Self::Dispi => DispiState::BYTE_LEN,
                Self::Et4000AxEv8913 => Et4000State::BYTE_LEN,
            },
        ]
    }
}

/// Relocade's opaque, versioned buffer. Relative offsets allow guest relocation.
struct BufferLayout {
    profile: Profile,
    components: u16,
    offsets: [u16; 4],
    bytes: u16,
}

impl BufferLayout {
    fn new(profile: Profile, components: u16) -> Option<Self> {
        if components == 0 || components & !0x0f != 0 {
            return None;
        }
        let mut offsets = [0; 4];
        let mut bytes = HEADER_SIZE as u16;
        for (index, size) in profile.component_sizes().into_iter().enumerate() {
            if components & (1 << index) != 0 {
                offsets[index] = bytes;
                bytes += size as u16;
            }
        }
        Some(Self {
            profile,
            components,
            offsets,
            bytes,
        })
    }

    fn header(&self) -> [u8; HEADER_SIZE] {
        let mut header = [0; HEADER_SIZE];
        header[..5].copy_from_slice(b"RVBE\x01");
        header[5] = self.profile as u8;
        header[6..8].copy_from_slice(&self.components.to_le_bytes());
        for (index, offset) in self.offsets.into_iter().enumerate() {
            header[8 + index * 2..10 + index * 2].copy_from_slice(&offset.to_le_bytes());
        }
        header
    }

    fn from_header(profile: Profile, header: &[u8; HEADER_SIZE]) -> Option<Self> {
        let components = u16::from_le_bytes(header[6..8].try_into().unwrap());
        let layout = Self::new(profile, components)?;
        (layout.header() == *header).then_some(layout)
    }

    fn component(&self, pointer: FarPointer, index: usize) -> FarPointer {
        pointer.wrapping_add(usize::from(self.offsets[index]))
    }
}

#[interrupt(context(call), args(operation = dl))]
pub(in crate::video::firmware) async fn dispatch(
    call: &mut CpuContext<'_>,
    card: &VideoFirmware,
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
fn required_blocks(card: &VideoFirmware, components: u16) -> Option<u16> {
    card.ports_enabled().then_some(())?;
    Some(
        BufferLayout::new(Profile::of(card)?, components)?
            .bytes
            .div_ceil(64),
    )
}

#[interrupt(context(memory), args(components = cx, destination = ptr(es, bx)),
    returns { Some(()) => { ax = 0x004f }, None => { ax = 0x014f } },
)]
fn save(
    memory: &mut MemoryView<'_>,
    card: &VideoFirmware,
    components: u16,
    destination: FarPointer,
) -> Option<()> {
    card.ports_enabled().then_some(())?;
    let layout = BufferLayout::new(Profile::of(card)?, components)?;
    // Capture everything before writing: the buffer may overlap BIOS data or VRAM.
    let (core, dac, extensions) = match card {
        VideoFirmware::Dispi(card) => {
            let card = card.borrow();
            (
                card.core_state().to_bytes(),
                card.dac_state().to_bytes(),
                card.dispi_state().to_bytes().to_vec(),
            )
        }
        VideoFirmware::Et4000Ax(card) => {
            let card = card.borrow();
            (
                card.core_state().to_bytes(),
                card.dac_state().to_bytes(),
                card.et4000_state().to_bytes().to_vec(),
            )
        }
        VideoFirmware::Vga => return None,
    };
    let firmware = (components & 2 != 0).then(|| firmware::save(memory));
    let blocks = [
        (components & 1 != 0).then_some(core.as_slice()),
        firmware.as_ref().map(|bytes| bytes.as_slice()),
        (components & 4 != 0).then_some(dac.as_slice()),
        (components & 8 != 0).then_some(extensions.as_slice()),
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
    card: &VideoFirmware,
    components: u16,
    source: FarPointer,
) -> Option<()> {
    card.ports_enabled().then_some(())?;
    let layout = BufferLayout::from_header(Profile::of(card)?, &read_buffer(memory, source))?;
    if components == 0 || components & !layout.components != 0 {
        return None;
    }
    // Validate all requested device classes before changing any state. Read BIOS
    // data before remapping VRAM or writing potentially overlapping destinations.
    let hardware = if components & 1 != 0 {
        let bytes = read_buffer(memory, layout.component(source, 0));
        Some(match card {
            VideoFirmware::Dispi(_) => VgaCoreState::from_bytes(&bytes)?,
            VideoFirmware::Et4000Ax(_) => Et4000Ax::decode_core_state(&bytes)?,
            VideoFirmware::Vga => return None,
        })
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
    match card {
        VideoFirmware::Dispi(card) => {
            let extensions = if components & 8 != 0 {
                Some(DispiState::from_bytes(&read_buffer(
                    memory,
                    layout.component(source, 3),
                ))?)
            } else {
                None
            };
            io.with_context(|context| {
                card.borrow_mut()
                    .restore_state(context, hardware, dac, extensions)
            });
        }
        VideoFirmware::Et4000Ax(card) => {
            let extensions = if components & 8 != 0 {
                Some(Et4000State::from_bytes(&read_buffer(
                    memory,
                    layout.component(source, 3),
                ))?)
            } else {
                None
            };
            io.with_context(|context| {
                card.borrow_mut()
                    .restore_state(context, hardware, dac, extensions)
            });
        }
        VideoFirmware::Vga => return None,
    }
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
