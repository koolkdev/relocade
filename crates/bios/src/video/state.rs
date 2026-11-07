//! INT 10h/AH=1Ch: IBM component buffers, separate from display captures.
mod dac;
pub(in crate::video) mod firmware;
mod hardware;

use relocade_machine::{real_mode_interrupt::interrupt, FarPointer, IoBus, MemoryView};

const HEADER_SIZE: u16 = 0x20;
const COMPONENT_SIZES: [u16; 3] = [
    hardware::SIZE as u16,
    firmware::SIZE as u16,
    dac::SIZE as u16,
];

/// Returns the required buffer size in 64-byte blocks, or None for an empty mask.
#[interrupt(args(components = cx), returns {
    Some(blocks) => { bx = blocks, al = 0x1c }, None => { al = 0 },
})]
pub(super) fn required_blocks(components: u16) -> Option<u16> {
    let components = components & 7;
    if components == 0 {
        return None;
    }
    let size = COMPONENT_SIZES
        .iter()
        .enumerate()
        .filter(|(index, _)| components & (1 << index) != 0)
        .map(|(_, size)| size)
        .sum::<u16>()
        + HEADER_SIZE;
    Some(size.div_ceil(64))
}

#[interrupt(context(memory, io), args(components = cx, destination = ptr(es, bx)),
    returns { true => { al = 0x1c }, false => { al = 0 } },
)]
pub(super) fn save(
    memory: &mut MemoryView<'_>,
    io: &IoBus,
    components: u16,
    destination: FarPointer,
) -> bool {
    let components = components & 7;
    if components == 0 {
        return false;
    }
    // Collect before writing: the guest buffer may overlap BIOS data.
    let hardware = (components & 1 != 0).then(|| hardware::save(memory, io));
    let firmware = (components & 2 != 0).then(|| firmware::save(memory));
    let dac = (components & 4 != 0).then(|| dac::save(io));
    let blocks = [
        hardware.as_ref().map(|bytes| bytes.as_slice()),
        firmware.as_ref().map(|bytes| bytes.as_slice()),
        dac.as_ref().map(|bytes| bytes.as_slice()),
    ];
    let mut payload = destination.wrapping_add(usize::from(HEADER_SIZE));
    for (index, block) in blocks.into_iter().enumerate() {
        if let Some(bytes) = block {
            // Header pointers are absolute ES offsets, not buffer displacements.
            memory
                .write_wrapping(
                    destination.wrapping_add(index * 2),
                    &payload.offset.to_le_bytes(),
                )
                .expect("video state header");
            memory
                .write_wrapping(payload, bytes)
                .expect("video state buffer");
            payload = payload.wrapping_add(bytes.len());
        }
    }
    true
}

#[interrupt(context(memory, io), args(components = cx, source = ptr(es, bx)),
    returns { true => { al = 0x1c }, false => { al = 0 } },
)]
pub(super) fn restore(
    memory: &mut MemoryView<'_>,
    io: &IoBus,
    components: u16,
    source: FarPointer,
) -> bool {
    let components = components & 7;
    if components == 0 {
        return false;
    }
    // Read every selected block before restoring BIOS data or remapping VRAM.
    // A subset restore follows the saved pointers, not a newly packed layout.
    let hardware = (components & 1 != 0).then(|| read_component(memory, source));
    let firmware = (components & 2 != 0).then(|| read_component(memory, source.wrapping_add(2)));
    let dac = (components & 4 != 0).then(|| read_component(memory, source.wrapping_add(4)));
    if let Some(bytes) = hardware {
        hardware::restore(memory, io, &bytes);
    }
    if let Some(bytes) = firmware {
        firmware::restore(memory, &bytes);
    }
    if let Some(bytes) = dac {
        dac::restore(io, &bytes);
    }
    true
}

// Unsupported operations leave the buffer and carry unchanged.
#[interrupt(returns { () => { al = 0 } })]
pub(super) fn unsupported() {}

fn read_component<const N: usize>(memory: &MemoryView<'_>, pointer: FarPointer) -> [u8; N] {
    let offset = u16::from_le_bytes(read_buffer(memory, pointer));
    read_buffer(memory, FarPointer { offset, ..pointer })
}

fn read_buffer<const N: usize>(memory: &MemoryView<'_>, pointer: FarPointer) -> [u8; N] {
    let mut bytes = [0; N];
    memory
        .read_wrapping(pointer, &mut bytes)
        .expect("video state buffer");
    bytes
}
