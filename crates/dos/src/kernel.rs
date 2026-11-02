use relocade_machine::{CpuConfig, MachineConfig};
use relocade_x86::{
    Compiler, CpuState, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap, Segments,
};

pub(crate) fn configuration() -> MachineConfig {
    const CONVENTIONAL_RAM_SIZE: u32 = 640 * 1024;
    const MEMORY_SIZE: u32 = 1024 * 1024;
    let kernel_rom_start = 0xf0000;
    let map = PhysicalMemoryMap::new([
        (
            0..=CONVENTIONAL_RAM_SIZE - 1,
            PhysicalMapping::Ram { backing_offset: 0 },
        ),
        (relocade_vga::MEMORY_RANGE, PhysicalMapping::Mmio),
        (
            kernel_rom_start..=MEMORY_SIZE - 1,
            PhysicalMapping::Rom {
                backing_offset: kernel_rom_start,
            },
        ),
    ])
    .expect("fixed DOS memory map");
    MachineConfig::new(u64::from(MEMORY_SIZE), map).expect("fixed DOS memory map")
}

pub(crate) fn cpu() -> CpuConfig {
    CpuConfig::new(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            segments: Segments::real_mode(),
            ..CpuState::default()
        },
    )
    .expect("fixed real-mode DOS CPU")
}
