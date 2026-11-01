//! BIOS adapter metadata. Identification never changes VGA registers or memory.
use super::{unsupported, MODESET_CONTROL, VIDEO_CONTROL, VIDEO_SWITCHES};
use relocade_machine::MemoryView;

const EQUIPMENT: u32 = 0x410;
const DISPLAY_COMBINATION: u32 = 0x48a;

// IBM's display-combination table. Each entry describes a pair of adapters;
// the equipment word determines which is active when both are present.
// Codes: 0 none, 1 MDA, 2 CGA, 4/5 EGA color/mono, 6 PGA, 7/8 VGA mono/color.
// These are BIOS descriptions, not a request to instantiate another device.
const DISPLAY_COMBINATIONS: [[u8; 2]; 16] = [
    [0, 0],
    [0, 1],
    [0, 2],
    [2, 1],
    [0, 4],
    [4, 1],
    [0, 5],
    [2, 5],
    [0, 6],
    [1, 6],
    [5, 6],
    [0, 8],
    [1, 8],
    [0, 7],
    [2, 7],
    [6, 7],
];

pub(super) fn initialize(memory: &mut MemoryView<'_>) {
    // 256 KiB, color adapter, and cursor emulation disabled by default.
    memory.write_u8(VIDEO_CONTROL, 0x61).expect("video control");
    memory.write_u8(VIDEO_SWITCHES, 0).expect("video switches");
    // All VGA modes are available regardless of the display description.
    memory
        .write_u8(MODESET_CONTROL, 1)
        .expect("mode-set control");
    memory
        .write_u8(DISPLAY_COMBINATION, 0x0b)
        .expect("display combination");
}

/// BIOS memory-size code: 0–3 represents 64–256 KiB.
pub(super) fn memory_size(memory: &MemoryView<'_>) -> u8 {
    (memory.read_u8(VIDEO_CONTROL).expect("video control") >> 5) & 3
}

pub(super) fn configuration(call: &mut crate::BiosContext<'_>) {
    let control = call.memory.read_u8(VIDEO_CONTROL).expect("video control");
    let switches = call.memory.read_u8(VIDEO_SWITCHES).expect("video switches");
    let size = memory_size(&call.memory);
    call.cpu.set_bh((control >> 1) & 1);
    call.cpu.set_bl(size);
    call.cpu.set_ch(switches >> 4);
    call.cpu.set_cl(switches & 15);
    call.cpu.set_al(size); // IBM VGA's AH=12h/BL=10h also leaves this value in AL.
}

/// Active and alternate display codes, independent of the current BIOS mode.
pub(super) fn display_codes(memory: &MemoryView<'_>) -> [u8; 2] {
    let index = memory
        .read_u8(DISPLAY_COMBINATION)
        .expect("display combination");
    let Some(mut codes) = DISPLAY_COMBINATIONS.get(usize::from(index)).copied() else {
        return [0xff; 2];
    };
    if codes[0] == 0 {
        codes.swap(0, 1);
    }
    if codes[1] != 0 {
        let monochrome = memory.read_u8(EQUIPMENT).expect("equipment flags") & 0x30 == 0x30;
        if (codes[0] & 1 != 0) != monochrome {
            codes.swap(0, 1);
        }
    }
    codes
}

pub(super) fn display_combination(call: &mut crate::BiosContext<'_>) {
    match call.cpu.al() {
        0 => call
            .cpu
            .set_bx(u16::from_le_bytes(display_codes(&call.memory))),
        1 => {
            let codes = [call.cpu.bl(), call.cpu.bh()];
            let index = DISPLAY_COMBINATIONS
                .iter()
                .position(|&entry| entry == codes || entry == [codes[1], codes[0]]);
            // IBM records an unknown combination for pairs absent from its table.
            call.memory
                .write_u8(DISPLAY_COMBINATION, index.map_or(0xff, |i| i as u8))
                .expect("display combination");
        }
        _ => return unsupported(call),
    }
    call.cpu.set_al(0x1a);
}
