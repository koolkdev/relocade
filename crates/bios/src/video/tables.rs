//! Guest-replaceable video tables (IBM BIOS Interface, April 1987, figures 3-20/21).
use super::{adapter, parameters, presets};
use crate::BIOS_SEGMENT;
use relocade_machine::{FarPointer, MemoryView};

const SAVE_POINTER: u32 = 0x4a8;
const PRIMARY: FarPointer = FarPointer {
    segment: BIOS_SEGMENT,
    offset: 0xa620,
};
const SECONDARY: FarPointer = FarPointer {
    segment: BIOS_SEGMENT,
    offset: 0xa640,
};
const DISPLAY_COMBINATIONS: FarPointer = FarPointer {
    segment: BIOS_SEGMENT,
    offset: 0xa660,
};
const PARAMETERS: FarPointer = FarPointer {
    segment: BIOS_SEGMENT,
    offset: 0xa700,
};
const LEGACY_PARAMETERS: FarPointer = FarPointer {
    segment: BIOS_SEGMENT,
    offset: 0xa690,
};

// INT 1Dh retains the PC BIOS's separate 6845 table: four register sets, buffer
// lengths, columns and mode controls. VGA mode setting uses the 4A8h table instead.
#[rustfmt::skip]
const LEGACY: [u8; 88] = [
    // 6845 profiles: 40-column, 80-column, CGA graphics, monochrome text.
    0x38, 0x28, 0x2d, 0x0a, 0x1f, 6, 0x19, 0x1c, 2, 7, 6, 7, 0, 0, 0, 0,
    0x71, 0x50, 0x5a, 0x0a, 0x1f, 6, 0x19, 0x1c, 2, 7, 6, 7, 0, 0, 0, 0,
    0x38, 0x28, 0x2d, 0x0a, 0x7f, 6, 0x64, 0x70, 2, 1, 6, 7, 0, 0, 0, 0,
    0x61, 0x50, 0x52, 0x0f, 0x19, 6, 0x19, 0x19, 2, 0x0d, 0x0b, 0x0c, 0, 0, 0, 0,
    0, 8, 0, 0x10, 0, 0x40, 0, 0x40, // Buffer sizes, little-endian words.
    40, 40, 80, 80, 40, 40, 80, 80, // Columns by mode.
    0x2c, 0x28, 0x2d, 0x29, 0x2a, 0x2e, 0x1e, 0x29, // Legacy mode controls.
];

pub(super) fn initialize(memory: &mut MemoryView<'_>) {
    let mut primary = [0; 28];
    primary[..4].copy_from_slice(&PARAMETERS.packed().to_le_bytes());
    primary[16..20].copy_from_slice(&SECONDARY.packed().to_le_bytes());
    memory
        .write_backing(PRIMARY.address(), &primary)
        .expect("video save-pointer table");
    let mut secondary = [0; 26];
    secondary[..2].copy_from_slice(&26u16.to_le_bytes());
    secondary[2..6].copy_from_slice(&DISPLAY_COMBINATIONS.packed().to_le_bytes());
    memory
        .write_backing(SECONDARY.address(), &secondary)
        .expect("secondary video save-pointer table");
    adapter::initialize_table(memory, DISPLAY_COMBINATIONS);
    memory
        .write_backing(LEGACY_PARAMETERS.address(), &LEGACY)
        .expect("legacy video parameters");
    memory
        .write_u32(0x1d * 4, LEGACY_PARAMETERS.packed())
        .expect("legacy video parameter vector");
    for (index, preset) in presets::TABLE.iter().enumerate() {
        let bytes = preset.map_or([0; parameters::SIZE], |preset| preset.parameters().encode());
        memory
            .write_backing(
                PARAMETERS.wrapping_add(index * parameters::SIZE).address(),
                &bytes,
            )
            .expect("BIOS video mode table");
    }
    memory
        .write_u32(SAVE_POINTER, PRIMARY.packed())
        .expect("video save pointer");
}

/// A table address in the BIOS save-pointer hierarchy rooted at BDA 4A8h.
/// Optional slots start null; guests install a table by writing its far pointer.
pub(super) enum Pointer {
    Parameters,
    DisplayCombinations,
    TextFont,
    GraphicsFont,
    AlternateFont,
}

impl Pointer {
    /// Resolve the current table address. A null pointer at any level means absent.
    pub fn read(self, memory: &MemoryView<'_>) -> Option<FarPointer> {
        let primary = non_null(memory.read_u32(SAVE_POINTER).expect("video save pointer"))?;
        let address = match self {
            Self::Parameters => primary,
            Self::TextFont => primary.wrapping_add(8),
            Self::GraphicsFont => primary.wrapping_add(12),
            Self::DisplayCombinations => {
                read_pointer(memory, primary.wrapping_add(16))?.wrapping_add(2)
            }
            Self::AlternateFont => read_pointer(memory, primary.wrapping_add(16))?.wrapping_add(6),
        };
        read_pointer(memory, address)
    }

    /// Return an override header when its table is installed and lists `mode`.
    /// The list follows the `HEADER_BYTES`-byte header and ends at FFh.
    pub fn get_override_header<const HEADER_BYTES: usize>(
        self,
        memory: &MemoryView<'_>,
        mode: u8,
    ) -> Option<[u8; HEADER_BYTES]> {
        let table = self.read(memory)?;
        let mode_list = table.wrapping_add(HEADER_BYTES);
        if !mode_list_contains(memory, mode_list, mode) {
            return None;
        }
        let mut header = [0; HEADER_BYTES];
        memory
            .read_wrapping(table, &mut header)
            .expect("video table header");
        Some(header)
    }
}

fn mode_list_contains(memory: &MemoryView<'_>, list: FarPointer, mode: u8) -> bool {
    // A malformed list must not loop forever after its offset wraps.
    for offset in 0..0x10000 {
        match memory
            .read_u8(list.wrapping_add(offset).address())
            .expect("video table modes")
        {
            0xff => return false,
            value if value == mode => return true,
            _ => {}
        }
    }
    false
}

fn read_pointer(memory: &MemoryView<'_>, address: FarPointer) -> Option<FarPointer> {
    let mut bytes = [0; 4];
    memory
        .read_wrapping(address, &mut bytes)
        .expect("video table pointer");
    non_null(u32::from_le_bytes(bytes))
}

fn non_null(packed: u32) -> Option<FarPointer> {
    (packed != 0).then(|| FarPointer::from_packed(packed))
}
