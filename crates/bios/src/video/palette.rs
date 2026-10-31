//! BIOS palette services through the guest-visible VGA ports.
mod compatibility;
use super::{buffer_addresses, modes, unsupported, MODE};
pub(super) use compatibility::{
    initialize as initialize_compatibility, interrupt as compatibility,
};
use relocade_machine::{real_mode_interrupt, CpuContext, HookResult, IoBus};
use relocade_vga::registers::{
    attribute, ATTRIBUTE_ADDRESS_PORT, ATTRIBUTE_DATA_READ_PORT, COLOR_INPUT_STATUS_1_PORT,
    DAC_DATA_PORT, DAC_MASK_PORT, DAC_READ_ADDRESS_PORT, DAC_WRITE_ADDRESS_PORT,
    MISC_OUTPUT_READ_PORT, MONO_INPUT_STATUS_1_PORT,
};

pub(super) fn interrupt(call: &mut CpuContext<'_>) -> HookResult {
    let function = call.cpu.al();
    let bl = call.cpu.bl();
    let bh = call.cpu.bh();
    let io = call.io;
    match function {
        0x00 | 0x07 if bl <= attribute::COLOR_SELECT => {
            if function == 0 {
                write_attribute(io, bl, bh);
            } else {
                call.cpu.set_bh(read_attribute(io, bl));
            }
        }
        0x01 => write_attribute(io, attribute::OVERSCAN_COLOR, bh),
        0x08 => call
            .cpu
            .set_bh(read_attribute(io, attribute::OVERSCAN_COLOR)),
        0x02 | 0x09 => {
            let base = call.cpu.segments().es.base;
            for (index, address) in buffer_addresses(base, call.cpu.dx(), 17).enumerate() {
                let register = if index == 16 {
                    attribute::OVERSCAN_COLOR
                } else {
                    index as u8
                };
                if function == 0x02 {
                    let value = call
                        .memory
                        .read_u8(address)
                        .expect("attribute palette buffer");
                    write_attribute(io, register, value);
                } else {
                    call.memory
                        .write_u8(address, read_attribute(io, register))
                        .expect("attribute palette buffer");
                }
            }
        }
        0x03 if bl <= 1 => {
            let mode = read_attribute(io, attribute::MODE_CONTROL);
            write_attribute(io, attribute::MODE_CONTROL, (mode & !8) | (bl << 3));
        }
        0x10 => {
            io.write_port(DAC_WRITE_ADDRESS_PORT, 1, u32::from(bl));
            for component in [call.cpu.dh(), call.cpu.ch(), call.cpu.cl()] {
                io.write_port(DAC_DATA_PORT, 1, u32::from(component));
            }
        }
        0x15 => {
            io.write_port(DAC_READ_ADDRESS_PORT, 1, u32::from(bl));
            call.cpu.set_dh(io.read_port(DAC_DATA_PORT, 1) as u8);
            call.cpu.set_ch(io.read_port(DAC_DATA_PORT, 1) as u8);
            call.cpu.set_cl(io.read_port(DAC_DATA_PORT, 1) as u8);
        }
        0x12 | 0x17 if call.cpu.cx() != 0 => {
            let reading = function == 0x17;
            let port = if reading {
                DAC_READ_ADDRESS_PORT
            } else {
                DAC_WRITE_ADDRESS_PORT
            };
            io.write_port(port, 1, u32::from(bl));
            let base = call.cpu.segments().es.base;
            let bytes = u32::from(call.cpu.cx()) * 3;
            for address in buffer_addresses(base, call.cpu.dx(), bytes) {
                if reading {
                    call.memory
                        .write_u8(address, io.read_port(DAC_DATA_PORT, 1) as u8)
                        .expect("DAC palette buffer");
                } else {
                    let component = call.memory.read_u8(address).expect("DAC palette buffer");
                    io.write_port(DAC_DATA_PORT, 1, u32::from(component));
                }
            }
        }
        0x12 | 0x17 => {} // Zero-length transfers do not disturb DAC access state.
        0x13 => {
            if call.memory.read_u8(MODE).expect("video mode") == modes::GRAPHICS_320X200_256 {
                return unsupported(call);
            }
            match bl {
                0 if bh <= 1 => {
                    let mode = read_attribute(io, attribute::MODE_CONTROL);
                    write_attribute(io, attribute::MODE_CONTROL, (mode & !0x80) | (bh << 7));
                }
                1 => {
                    let mode = read_attribute(io, attribute::MODE_CONTROL);
                    let page = if mode & 0x80 != 0 {
                        bh & 15
                    } else {
                        (bh & 3) << 2
                    };
                    write_attribute(io, attribute::COLOR_SELECT, page);
                }
                _ => return unsupported(call),
            }
        }
        0x18 => io.write_port(DAC_MASK_PORT, 1, u32::from(bl)),
        0x19 => call.cpu.set_bl(io.read_port(DAC_MASK_PORT, 1) as u8),
        0x1a => {
            let mode = read_attribute(io, attribute::MODE_CONTROL) >> 7;
            let page = read_attribute(io, attribute::COLOR_SELECT);
            call.cpu.set_bl(mode);
            call.cpu.set_bh(if mode == 0 { page >> 2 } else { page });
        }
        _ => return unsupported(call),
    }
    real_mode_interrupt::return_from_interrupt(call)
}

fn select_attribute(io: &IoBus, address: u8) {
    let status = if io.read_port(MISC_OUTPUT_READ_PORT, 1) & 1 != 0 {
        COLOR_INPUT_STATUS_1_PORT
    } else {
        MONO_INPUT_STATUS_1_PORT
    };
    io.read_port(status, 1); // Reset the address/data flip-flop.
    io.write_port(ATTRIBUTE_ADDRESS_PORT, 1, u32::from(address));
}

fn read_attribute(io: &IoBus, index: u8) -> u8 {
    let saved_address = io.read_port(ATTRIBUTE_ADDRESS_PORT, 1) as u8;
    select_attribute(io, index);
    let value = io.read_port(ATTRIBUTE_DATA_READ_PORT, 1) as u8;
    select_attribute(io, saved_address);
    value
}

fn write_attribute(io: &IoBus, index: u8, value: u8) {
    let saved_address = io.read_port(ATTRIBUTE_ADDRESS_PORT, 1) as u8;
    // Selecting a palette index clears PAS so Type-1 VGA accepts the write.
    select_attribute(io, index);
    io.write_port(ATTRIBUTE_ADDRESS_PORT, 1, u32::from(value));
    select_attribute(io, saved_address); // Preserve display enable and index.
}
