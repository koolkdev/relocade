use relocade_sound_blaster::{
    FM_ADDRESS_STATUS_PORT, FM_DATA_PORT, READ_DATA_PORT, READ_STATUS_PORT, RESET_PORT,
    WRITE_DATA_PORT,
};

const HANDLER: u16 = 0x400;
pub const STAGE: u16 = 0x700;
pub const IRQ_COUNT: u16 = 0x701;
pub const RESET_RESPONSE: u16 = 0x70b;
pub const BUFFER: u16 = 0x800;

fn out(code: &mut Vec<u8>, port: u16, value: u8) {
    code.extend_from_slice(&[0xba, port as u8, (port >> 8) as u8, 0xb0, value, 0xee]);
}

fn fm(code: &mut Vec<u8>, register: u8, value: u8) {
    out(code, FM_ADDRESS_STATUS_PORT, register);
    out(code, FM_DATA_PORT, value);
}

#[rustfmt::skip]
fn pause(code: &mut Vec<u8>, stage: u8) {
    code.extend_from_slice(&[
        0xc6, 6, STAGE as u8, (STAGE >> 8) as u8, stage,
        0xb4, 7, 0xcd, 0x21, // DOS read without echo.
    ]);
}

// Keep the authored instructions grouped by operation.
#[rustfmt::skip]
pub fn image() -> Vec<u8> {
    let mut code = vec![
        0xc6, 6, IRQ_COUNT as u8, (IRQ_COUNT >> 8) as u8, 0,
        0xc7, 6, 2, 7, BUFFER as u8, (BUFFER >> 8) as u8, // Next half to refill.
        0xc6, 6, 4, 7, 0x60, // Next refill byte; alternate 60h/A0h.
    ];
    out(&mut code, RESET_PORT, 1);
    out(&mut code, RESET_PORT, 0);
    pause(&mut code, 1);
    code.extend_from_slice(&[
        0xba, READ_STATUS_PORT as u8, (READ_STATUS_PORT >> 8) as u8,
        0xec, 0xa8, 0x80, 0x74, 0xfb, // Wait for reset response.
        0xba, READ_DATA_PORT as u8, (READ_DATA_PORT >> 8) as u8, 0xec,
        0xa2, RESET_RESPONSE as u8, (RESET_RESPONSE >> 8) as u8,
        0xb8, 0x0f, 0x35, 0xcd, 0x21, // Get IRQ7 vector through DOS.
        0x89, 0x1e, 6, 7, 0x8c, 6, 8, 7, // Save BX/ES at 0706h/0708h.
        0xba, HANDLER as u8, (HANDLER >> 8) as u8,
        0xb8, 0x0f, 0x25, 0xcd, 0x21, // Set IRQ7 vector to DS:handler.
        0xe4, relocade_pic::MASK_PORT as u8, 0xa2, 0x0a, 7,
        0x24, 0x7f, 0xe6, relocade_pic::MASK_PORT as u8, // Save mask, enable IRQ7.
        0x1e, 0x07, 0xfc, // ES = DS, CLD.
        0xbf, BUFFER as u8, (BUFFER >> 8) as u8,
        0xb9, 32, 0, 0xb0, 0x40, 0xf3, 0xaa, // First half: -0.5.
        0xb9, 32, 0, 0xb0, 0xc0, 0xf3, 0xaa, // Second half: +0.5.
    ]);
    use relocade_dma::*;
    out(&mut code, SINGLE_MASK_PORT, 5);
    out(&mut code, CLEAR_FLIP_FLOP_PORT, 0);
    code.extend_from_slice(&[
        0x8c, 0xd8, 0xc1, 0xe0, 4, // AX = low word of DS * 16.
        0x05, BUFFER as u8, (BUFFER >> 8) as u8,
        0xba, ADDRESS_PORTS[1] as u8, 0, 0xee, 0x88, 0xe0, 0xee,
    ]);
    out(&mut code, COUNT_PORTS[1], 63);
    out(&mut code, COUNT_PORTS[1], 0);
    code.extend_from_slice(&[
        0x8c, 0xd8, 0xc1, 0xe8, 12, // AX = high byte of the physical address.
        0xe6, PAGE_PORTS[1] as u8,
    ]);
    out(&mut code, MODE_PORT, 0x59); // Channel 1, memory read, auto-initialize.
    out(&mut code, SINGLE_MASK_PORT, 1);
    for (register, value) in [
        (0x20, 0x21), (0x23, 0x21), (0x40, 0x3f), (0x43, 0),
        (0x60, 0xf0), (0x63, 0xf0), (0x80, 0x0f), (0x83, 0x0f),
        (0xa0, 0x98), (0xb0, 0x31), // Sustained ~309 Hz OPL2 carrier.
    ] {
        fm(&mut code, register, value);
    }
    code.extend_from_slice(&[
        0xb0, 0xb6, 0xe6, 0x43, // PIT channel 2, square wave.
        0xb0, 0xa9, 0xe6, 0x42, 0xb0, 4, 0xe6, 0x42, // ~1 kHz.
        0xb0, 3, 0xe6, 0x61,
    ]);
    for value in [0xd1, 0x40, 131, 0x48, 31, 0, 0x1c] {
        out(&mut code, WRITE_DATA_PORT, value); // 8 kHz, 32-byte DSP blocks.
    }
    pause(&mut code, 2);
    out(&mut code, RESET_PORT, 1);
    out(&mut code, RESET_PORT, 0);
    fm(&mut code, 0xb0, 0x11);
    code.extend_from_slice(&[
        0xb0, 0, 0xe6, 0x61, // Disable speaker.
        0xa0, 0x0a, 7, 0xe6, relocade_pic::MASK_PORT as u8, // Restore PIC mask.
        0x8b, 0x16, 6, 7, 0xa1, 8, 7, // DX/AX = saved vector offset/segment.
        0x1e, 0x8e, 0xd8, 0xb8, 0x0f, 0x25, 0xcd, 0x21, 0x1f,
        0xb8, 0, 0x4c, 0xcd, 0x21,
    ]);
    assert!(code.len() <= usize::from(HANDLER - 0x100));
    code.resize(usize::from(HANDLER - 0x100), 0);
    code.extend_from_slice(&[
        0x50, 0x51, 0x52, 0x57, 0x1e, 0x06, // Preserve AX/CX/DX/DI/DS/ES.
        0x0e, 0x1f, 0x0e, 0x07, 0xfc, // DS = ES = CS, CLD.
        0xba, READ_STATUS_PORT as u8, (READ_STATUS_PORT >> 8) as u8, 0xec,
        0x8b, 0x3e, 2, 7, 0xb9, 32, 0, 0xa0, 4, 7, 0xf3, 0xaa, // Refill half.
        0x83, 0x36, 2, 7, 32, // Alternate buffer halves.
        0x80, 0x36, 4, 7, 0xc0, // Alternate refill levels.
        0xfe, 6, IRQ_COUNT as u8, (IRQ_COUNT >> 8) as u8,
        0xb0, 0x20, 0xe6, relocade_pic::COMMAND_PORT as u8, // PIC EOI.
        0x07, 0x1f, 0x5f, 0x5a, 0x59, 0x58, 0xcf,
    ]);
    code
}
