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
