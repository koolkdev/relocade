use crate::{files::Files, memory, process, services, ServiceError};
use wasm86_machine::{MachineConfig, Memory};
use wasm86_runtime::Hook;
use wasm86_x86::{
    Compiler, CpuState, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap, Segments,
};

const KERNEL_SEGMENT: u16 = 0xf000;
const INT20_ENTRY: u16 = 0x100;
const INT21_ENTRY: u16 = 0x110;

/// Mutable DOS state owned by the Wasm backend and shared by its service handlers.
/// Initialize the backend with `DosState::default()` before loading a program.
#[derive(Default)]
pub struct DosState {
    pub(crate) files: Files,
    pub(crate) dta_segment: u16,
    pub(crate) dta_offset: u16,
    pub(crate) output: Vec<u8>,
    pub(crate) exit_status: Option<Result<u8, ServiceError>>,
}

impl DosState {
    pub(crate) fn reset_process(&mut self, psp_segment: u16) {
        self.files.reset_handles();
        self.dta_segment = psp_segment;
        self.dta_offset = process::COMMAND_TAIL_OFFSET;
        self.output.clear();
        self.exit_status = None;
    }
}

pub(crate) fn configuration() -> MachineConfig {
    const CONVENTIONAL_RAM_SIZE: u32 = 640 * 1024;
    const MEMORY_SIZE: u32 = 1024 * 1024;
    let kernel_rom_start = address(0);
    let map = PhysicalMemoryMap::new([
        (
            0..=CONVENTIONAL_RAM_SIZE - 1,
            PhysicalMapping::Ram { backing_offset: 0 },
        ),
        (
            kernel_rom_start..=MEMORY_SIZE - 1,
            PhysicalMapping::Rom {
                backing_offset: kernel_rom_start,
            },
        ),
    ])
    .expect("fixed DOS memory map");
    MachineConfig::new(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            segments: Segments::real_mode(),
            ..CpuState::default()
        },
        u64::from(MEMORY_SIZE),
        map,
    )
    .expect("fixed real-mode DOS configuration")
}

pub(crate) fn initialize(memory: &mut Memory<'_>) {
    memory::reset(memory);
    for (vector, offset) in [(0x20, INT20_ENTRY), (0x21, INT21_ENTRY)] {
        let [lo, hi] = offset.to_le_bytes();
        let [seg_lo, seg_hi] = KERNEL_SEGMENT.to_le_bytes();
        memory
            .write(vector * 4, &[lo, hi, seg_lo, seg_hi])
            .expect("IVT range");
        // NOP reserves the hook's entry byte; returning services continue at IRET.
        memory
            .write_backing(address(offset), &[0x90, 0xcf])
            .expect("DOS kernel ROM range");
    }
}

pub(crate) fn hooks() -> [Hook<DosState>; 2] {
    [
        Hook::new(address(INT20_ENTRY), services::int20),
        Hook::new(address(INT21_ENTRY), services::int21),
    ]
}

const fn address(offset: u16) -> u32 {
    ((KERNEL_SEGMENT as u32) << 4) + offset as u32
}
