use crate::{console, ServiceError};
use std::ops::ControlFlow;
use wasm86_machine::{MachineConfig, Memory};
use wasm86_runtime::{Hook, HostCall};
use wasm86_x86::{
    Compiler, CpuState, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap, Segments,
};

const KERNEL_SEGMENT: u16 = 0xf000;
const INT20_ENTRY: u16 = 0x100;
const INT21_ENTRY: u16 = 0x110;
const WRITE_STRING: u8 = 0x09;
const WRITE_HANDLE: u8 = 0x40;
const TERMINATE_PROCESS: u8 = 0x4c;

/// Mutable DOS state owned by the Wasm backend and shared by its service handlers.
/// Initialize the backend with `DosState::default()` before loading a program.
#[derive(Default)]
pub struct DosState {
    pub(crate) output: Vec<u8>,
    pub(crate) exit_status: Option<Result<u8, ServiceError>>,
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
        Hook::new(address(INT20_ENTRY), int20),
        Hook::new(address(INT21_ENTRY), int21),
    ]
}

const fn address(offset: u16) -> u32 {
    ((KERNEL_SEGMENT as u32) << 4) + offset as u32
}

/// INT 21h selects a DOS service through AH.
fn int21(call: &mut HostCall<'_, DosState>) -> ControlFlow<()> {
    match call.cpu().ah() {
        WRITE_STRING => console::write_string(call),
        WRITE_HANDLE => console::write_handle(call),
        TERMINATE_PROCESS => terminate_process(call),
        function => return_to_host(call, Err(ServiceError::UnsupportedFunction(function))),
    }
}

/// AH=4Ch: terminate the current process with the exit code in AL.
fn terminate_process(call: &mut HostCall<'_, DosState>) -> ControlFlow<()> {
    let code = call.cpu().al();
    return_to_host(call, Ok(code))
}

/// INT 20h terminates without an exit code, including a COM program's initial RET.
fn int20(call: &mut HostCall<'_, DosState>) -> ControlFlow<()> {
    return_to_host(call, Ok(0))
}

pub(crate) fn return_to_host(
    call: &mut HostCall<'_, DosState>,
    status: Result<u8, ServiceError>,
) -> ControlFlow<()> {
    call.state_mut().exit_status = Some(status);
    ControlFlow::Break(())
}
