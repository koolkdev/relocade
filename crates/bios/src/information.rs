//! Machine information shared by the BIOS data area and INT 11h/12h.

use relocade_machine::{real_mode_interrupt::interrupt, FarPointer, MemoryView};

pub(crate) const EQUIPMENT: FarPointer = FarPointer {
    segment: 0x0040,
    offset: 0x10,
};
const MEMORY_SIZE: FarPointer = FarPointer {
    segment: 0x0040,
    offset: 0x13,
};
const MATH_COPROCESSOR: u16 = 1 << 1;
pub(crate) const DISPLAY_MASK: u8 = 0x30;
pub(crate) const MONOCHROME_DISPLAY: u8 = 0x30;

/// Hardware facts supplied by the PC when initializing its BIOS.
/// The video BIOS supplies VGA; diskette and serial equipment fields start clear.
pub struct Configuration {
    pub conventional_memory_kib: u16,
    pub math_coprocessor: bool,
    pub pointing_device: bool,
    /// Base addresses in BIOS LPT order. Zero marks an absent port; an installed
    /// adapter counts as equipment even when its connector is empty.
    pub parallel_ports: [u16; 3],
}

pub(crate) fn initialize(memory: &mut MemoryView<'_>, configuration: Configuration) {
    // Display bits 00 identify EGA/VGA. An x87 is independent of peripheral IRQs.
    let parallel_count = configuration
        .parallel_ports
        .iter()
        .filter(|&&base| base != 0)
        .count() as u16;
    let equipment = (parallel_count << 14)
        | (u16::from(configuration.pointing_device) << 2)
        | if configuration.math_coprocessor {
            MATH_COPROCESSOR
        } else {
            0
        };
    memory
        .write_u16(EQUIPMENT.address(), equipment)
        .expect("BIOS equipment word");
    memory
        .write_u16(MEMORY_SIZE.address(), configuration.conventional_memory_kib)
        .expect("BIOS conventional memory size");
}

#[interrupt(context(memory), returns { equipment => { ax = equipment } })]
pub(crate) fn equipment(memory: &MemoryView<'_>) -> u16 {
    memory
        .read_u16(EQUIPMENT.address())
        .expect("BIOS equipment word")
}

#[interrupt(context(memory), returns { kib => { ax = kib } })]
pub(crate) fn conventional_memory(memory: &MemoryView<'_>) -> u16 {
    memory
        .read_u16(MEMORY_SIZE.address())
        .expect("BIOS conventional memory size")
}
