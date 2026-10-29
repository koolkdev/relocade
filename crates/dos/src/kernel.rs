use crate::{interrupt, memory, services, state::DosState};
use relocade_machine::{real_mode_interrupt::HOOK_STUB, Hook};
use relocade_machine::{CpuConfig, MachineConfig, MemoryView};
use relocade_x86::{
    Compiler, CpuState, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap, Segments,
};
use std::rc::Rc;

const KERNEL_SEGMENT: u16 = 0xf000;
const INT20_ENTRY: u16 = 0x100;
const INT21_ENTRY: u16 = 0x110;

pub(crate) fn configuration() -> MachineConfig {
    const CONVENTIONAL_RAM_SIZE: u32 = 640 * 1024;
    const MEMORY_SIZE: u32 = 1024 * 1024;
    let kernel_rom_start = address(0);
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

pub(crate) fn initialize(memory: &mut MemoryView<'_>) {
    memory::reset(memory);
    for (vector, offset) in [(0x20, INT20_ENTRY), (0x21, INT21_ENTRY)] {
        let [lo, hi] = offset.to_le_bytes();
        let [seg_lo, seg_hi] = KERNEL_SEGMENT.to_le_bytes();
        memory
            .write(vector * 4, &[lo, hi, seg_lo, seg_hi])
            .expect("IVT range");
        memory
            .write_backing(address(offset), &HOOK_STUB)
            .expect("DOS kernel ROM range");
    }
}

pub(crate) fn hooks(state: Rc<DosState>) -> [Hook; 2] {
    [
        interrupt::hook(address(INT20_ENTRY), Rc::clone(&state), services::int20),
        interrupt::hook(address(INT21_ENTRY), state, services::int21),
    ]
}

const fn address(offset: u16) -> u32 {
    ((KERNEL_SEGMENT as u32) << 4) + offset as u32
}
