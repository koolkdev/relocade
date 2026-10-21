//! Initial machine configuration and scoped access to live guest state.
//!
//! [`MachineConfig`] describes the initial CPU, backing size and fixed physical
//! map. Execution backends provide storage; [`CpuContext`] borrows that storage to
//! expose the current CPU and [`Memory`] with the machine's access policy.
//!
//! ```
//! use relocade_machine::MachineConfig;
//! use relocade_x86::{Compiler, CpuState, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap, Segments};
//!
//! let target = Compiler::new(ExecutionProfile::Real16).target();
//! let cpu = CpuState { eip: 0x100, segments: Segments::real_mode(), ..CpuState::default() };
//! let map = PhysicalMemoryMap::new([(0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 })])?;
//! let config = MachineConfig::new(target, cpu, 2 * 1024 * 1024, map)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! Live access reads only the requested CPU fields. Use a snapshot explicitly
//! when saving or inspecting the complete CPU, including floating-point state.
//!
//! ```
//! # use relocade_machine::CpuContext;
//! # fn update(machine: &mut CpuContext<'_>) -> Result<(), Box<dyn std::error::Error>> {
//! let ax = machine.cpu().ax();
//! machine.memory().write(0x200, &ax.to_le_bytes())?;
//! machine.cpu().set_ax(ax.wrapping_add(1));
//! let saved = machine.cpu().snapshot();
//! machine.cpu().restore(&saved)?;
//! # Ok(())
//! # }
//! ```
#![forbid(unsafe_code)]

mod config;
mod cpu;
mod memory;

pub use config::{ConfigurationError, MachineConfig};
pub use cpu::{CpuContext, CpuView};
pub use memory::{Memory, MemoryError, ReadMemory};

/// Transfers to the CPU and guest memory supplied by the runtime. CPU offsets
/// are relative to the start of `CpuState` in the execution context.
/// Writes take effect immediately; implementations must not cache a CPU snapshot.
/// Backing transfers receive ranges already checked against configured capacity.
pub trait CpuStorage {
    fn read_cpu(&self, offset: u32, bytes: &mut [u8]);
    fn write_cpu(&mut self, offset: u32, bytes: &[u8]);
    fn read_backing(&self, offset: u32, bytes: &mut [u8]);
    fn write_backing(&mut self, offset: u32, bytes: &[u8]);
}
