use crate::CONVENTIONAL_MEMORY_SIZE;
use relocade_machine::{CpuConfig, MachineConfig};
use relocade_x86::{Compiler, CpuState, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap};

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
