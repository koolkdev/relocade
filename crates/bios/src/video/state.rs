//! INT 10h/AH=1Ch: IBM component buffers, separate from display captures.
mod dac;
mod firmware;
mod hardware;

use super::buffer_addresses;
use relocade_machine::MemoryView;

const HEADER_SIZE: u16 = 0x20;
const COMPONENT_SIZES: [u16; 3] = [
    hardware::SIZE as u16,
    firmware::SIZE as u16,
    dac::SIZE as u16,
];

pub(super) fn interrupt(call: &mut crate::BiosContext<'_>) {
    let components = call.cpu.cx() & 7;
    match call.cpu.al() {
        0 if components != 0 => {
            let size = COMPONENT_SIZES
                .iter()
                .enumerate()
                .filter(|(index, _)| components & (1 << index) != 0)
                .map(|(_, size)| size)
                .sum::<u16>()
                + HEADER_SIZE;
            call.cpu.set_bx(size.div_ceil(64));
        }
        1 if components != 0 => save(call, components),
        2 if components != 0 => restore(call, components),
        _ => {
            call.cpu.set_al(0);
            return;
        }
    }
    call.cpu.set_al(0x1c);
}

fn save(call: &mut crate::BiosContext<'_>, components: u16) {
    // Collect before writing: the guest buffer may overlap BIOS data.
    let hardware = (components & 1 != 0).then(|| hardware::save(&mut call.memory, call.io));
    let firmware = (components & 2 != 0).then(|| firmware::save(&call.memory));
    let dac = (components & 4 != 0).then(|| dac::save(call.io));
    let blocks = [
        hardware.as_ref().map(|bytes| bytes.as_slice()),
        firmware.as_ref().map(|bytes| bytes.as_slice()),
        dac.as_ref().map(|bytes| bytes.as_slice()),
    ];
    let base = call.cpu.segments().es.base;
    let header = call.cpu.bx();
    let mut offset = header.wrapping_add(HEADER_SIZE);
    for (index, block) in blocks.into_iter().enumerate() {
        if let Some(bytes) = block {
            // Header pointers are absolute ES offsets, not buffer displacements.
            write_buffer(
                &mut call.memory,
                base,
                header.wrapping_add(index as u16 * 2),
                &offset.to_le_bytes(),
            );
            write_buffer(&mut call.memory, base, offset, bytes);
            offset = offset.wrapping_add(bytes.len() as u16);
        }
    }
}

fn write_buffer(memory: &mut MemoryView<'_>, base: u32, offset: u16, bytes: &[u8]) {
    for (address, &byte) in buffer_addresses(base, offset, bytes.len() as u32).zip(bytes) {
        memory.write_u8(address, byte).expect("video state buffer");
    }
}

fn restore(call: &mut crate::BiosContext<'_>, components: u16) {
    let base = call.cpu.segments().es.base;
    let header = call.cpu.bx();
    // Read every selected block before restoring BIOS data or remapping VRAM.
    // A subset restore follows the saved pointers, not a newly packed layout.
    let hardware = (components & 1 != 0).then(|| read_component(&call.memory, base, header));
    let firmware =
        (components & 2 != 0).then(|| read_component(&call.memory, base, header.wrapping_add(2)));
    let dac =
        (components & 4 != 0).then(|| read_component(&call.memory, base, header.wrapping_add(4)));
    if let Some(bytes) = hardware {
        hardware::restore(&mut call.memory, call.io, &bytes);
    }
    if let Some(bytes) = firmware {
        firmware::restore(&mut call.memory, &bytes);
    }
    if let Some(bytes) = dac {
        dac::restore(call.io, &bytes);
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
