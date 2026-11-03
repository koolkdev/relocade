//! BIOS adapter metadata. Identification never changes VGA registers or memory.
use super::{tables, MODESET_CONTROL, VIDEO_CONTROL, VIDEO_SWITCHES};
use relocade_machine::{FarPointer, MemoryView};

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

pub(super) fn initialize_table(memory: &mut MemoryView<'_>, pointer: FarPointer) {
    let mut bytes = [0; 36];
    bytes[..4].copy_from_slice(&[16, 1, 8, 0]); // Count, version, highest code, reserved.
    for (entry, codes) in bytes[4..]
        .as_chunks_mut::<2>()
        .0
        .iter_mut()
        .zip(DISPLAY_COMBINATIONS)
    {
        entry.copy_from_slice(&codes);
    }
    memory
        .write_backing(pointer.address(), &bytes)
        .expect("BIOS display-combination table");
}

fn display_combinations(memory: &MemoryView<'_>) -> Vec<[u8; 2]> {
    let Some(pointer) = tables::Pointer::DisplayCombinations.read(memory) else {
        return Vec::new();
    };
    let count = memory
        .read_u8(pointer.address())
        .expect("display-combination count");
    let mut bytes = vec![0; usize::from(count) * 2];
    memory
        .read_wrapping(pointer.wrapping_add(4), &mut bytes)
        .expect("display-combination entries");
    bytes.as_chunks::<2>().0.to_vec()
}

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

pub(super) struct Configuration {
    pub monochrome: bool,
    pub memory_size: u8,
    pub features: u8,
    pub switches: u8,
}

#[relocade_machine::real_mode_interrupt::interrupt(context(memory), returns {
    config => {
        bh = u8::from(config.monochrome), bl = config.memory_size,
        ch = config.features, cl = config.switches,
        al = config.memory_size,
    },
})]
pub(super) fn configuration(memory: &MemoryView<'_>) -> Configuration {
    let control = memory.read_u8(VIDEO_CONTROL).expect("video control");
    let switches = memory.read_u8(VIDEO_SWITCHES).expect("video switches");
    Configuration {
        monochrome: control & 2 != 0,
        memory_size: memory_size(memory),
        features: switches >> 4,
        switches: switches & 15,
    }
}

/// Active and alternate display codes, independent of the current BIOS mode.
#[relocade_machine::real_mode_interrupt::interrupt(context(memory), returns {
    [active, alternate] => { bl = active, bh = alternate, al = 0x1a },
})]
pub(super) fn display_codes(memory: &MemoryView<'_>) -> [u8; 2] {
    let index = memory
        .read_u8(DISPLAY_COMBINATION)
        .expect("display combination");
    let Some(mut codes) = display_combinations(memory)
        .get(usize::from(index))
        .copied()
    else {
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

/// Stores the matching display-combination index, accepting either code order.
#[relocade_machine::real_mode_interrupt::interrupt(
    context(memory), args(active = bl, alternate = bh),
    returns { () => { al = 0x1a } },
)]
pub(super) fn set_display_codes(memory: &mut MemoryView<'_>, active: u8, alternate: u8) {
    let index = display_combinations(memory)
        .iter()
        .position(|&entry| entry == [active, alternate] || entry == [alternate, active]);
    // IBM records an unknown combination for pairs absent from its table.
    memory
        .write_u8(DISPLAY_COMBINATION, index.map_or(0xff, |i| i as u8))
        .expect("display combination");
}
