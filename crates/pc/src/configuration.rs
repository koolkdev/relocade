use crate::CONVENTIONAL_MEMORY_SIZE;
use relocade_machine::{CpuConfig, MachineConfig};
use relocade_x86::{Compiler, CpuState, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap};

/// Hardware installed before guest execution, independently of host playback.
#[derive(Clone, Copy, Debug, Default)]
pub struct Configuration {
    pub sound_card: SoundCard,
}

/// The installed sound card. The PC speaker is available with either choice.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SoundCard {
    None,
    #[default]
    AdLib,
}

pub fn memory() -> MachineConfig {
    const MEMORY_SIZE: u32 = 1024 * 1024;
    const BIOS_ROM_START: u32 = 0xf0000;
    let map = PhysicalMemoryMap::new([
        (
            0..=CONVENTIONAL_MEMORY_SIZE - 1,
            PhysicalMapping::Ram { backing_offset: 0 },
        ),
        (relocade_vga::MEMORY_RANGE, PhysicalMapping::MMIO),
        (
            BIOS_ROM_START..=MEMORY_SIZE - 1,
            PhysicalMapping::Rom {
                backing_offset: BIOS_ROM_START,
            },
        ),
    ])
    .expect("fixed PC memory map");
    MachineConfig::new(u64::from(MEMORY_SIZE), map).expect("fixed PC memory map")
}

pub fn cpu() -> CpuConfig {
    CpuConfig::new(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState::real_mode(),
    )
    .expect("fixed real-mode PC CPU")
}
