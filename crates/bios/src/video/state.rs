//! INT 10h/AH=1Ch: IBM component buffers, separate from display captures.
mod dac;
mod firmware;
mod hardware;

use super::buffer_addresses;
use relocade_machine::{
    real_mode_interrupt::{interrupt, FarPointer},
    IoBus, MemoryView,
};

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
    let base = destination.segment_base();
    let header = destination.offset;
    let mut offset = header.wrapping_add(HEADER_SIZE);
    for (index, block) in blocks.into_iter().enumerate() {
        if let Some(bytes) = block {
            // Header pointers are absolute ES offsets, not buffer displacements.
            write_buffer(
                memory,
                base,
                header.wrapping_add(index as u16 * 2),
                &offset.to_le_bytes(),
            );
            write_buffer(memory, base, offset, bytes);
            offset = offset.wrapping_add(bytes.len() as u16);
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
    let base = source.segment_base();
    let header = source.offset;
    // Read every selected block before restoring BIOS data or remapping VRAM.
    // A subset restore follows the saved pointers, not a newly packed layout.
    let hardware = (components & 1 != 0).then(|| read_component(memory, base, header));
    let firmware =
        (components & 2 != 0).then(|| read_component(memory, base, header.wrapping_add(2)));
    let dac = (components & 4 != 0).then(|| read_component(memory, base, header.wrapping_add(4)));
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

fn write_buffer(memory: &mut MemoryView<'_>, base: u32, offset: u16, bytes: &[u8]) {
    for (address, &byte) in buffer_addresses(base, offset, bytes.len() as u32).zip(bytes) {
        memory.write_u8(address, byte).expect("video state buffer");
    }
}

fn read_component<const N: usize>(memory: &MemoryView<'_>, base: u32, pointer: u16) -> [u8; N] {
    let offset = u16::from_le_bytes(read_buffer(memory, base, pointer));
    read_buffer(memory, base, offset)
}

fn read_buffer<const N: usize>(memory: &MemoryView<'_>, base: u32, offset: u16) -> [u8; N] {
    let mut bytes = [0; N];
    for (address, byte) in buffer_addresses(base, offset, N as u32).zip(&mut bytes) {
        *byte = memory.read_u8(address).expect("video state buffer");
    }
    bytes
}
