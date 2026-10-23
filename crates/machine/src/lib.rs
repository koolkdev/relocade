//! Initial machine configuration and scoped access to live guest state.
//!
//! [`CpuConfig`] describes initial CPU state. [`MachineConfig`] describes backing
//! size and the fixed physical map. Execution backends provide storage; [`CpuContext`] borrows that storage to
//! expose the current CPU and [`MemoryView`] with the machine's access policy.
//!
//! ```
//! use relocade_machine::{CpuConfig, MachineConfig};
//! use relocade_x86::{Compiler, CpuState, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap, Segments};
//!
//! let target = Compiler::new(ExecutionProfile::Real16).target();
//! let cpu = CpuState { eip: 0x100, segments: Segments::real_mode(), ..CpuState::default() };
//! let map = PhysicalMemoryMap::new([(0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 })])?;
//! let cpu = CpuConfig::new(target, cpu)?;
//! let config = MachineConfig::new(2 * 1024 * 1024, map)?;
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
pub mod runtime;

pub use config::{ConfigurationError, CpuConfig, MachineConfig};
pub use cpu::{CpuContext, CpuView};
pub use memory::{Memory, MemoryError, MemoryView};
pub use relocade_wasm_runtime::{MemoryAccess, MemoryStorage, SharedMemoryStorage};

#[cfg(feature = "worker")]
pub use runtime::worker;
pub use runtime::{
    real_mode_interrupt, Cpu, DispatchEntry, Error, Hook, HookResult, HostCall, Result,
    RuntimeBackend, RuntimeMemory, RuntimeState,
};
