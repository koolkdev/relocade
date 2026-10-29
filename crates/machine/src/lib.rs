//! Machine configuration, live state access and asynchronous guest execution.
//!
//! [`CpuConfig`] describes initial CPU state. [`MachineConfig`] describes backing
//! size and the fixed physical map. [`CpuContext`] borrows the machine and exposes
//! live CPU state and [`MemoryView`] with the machine's access policy. Views can
//! cross ordinary awaits while keeping the machine borrowed.
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
//! let cpu = &mut machine.cpu;
//! let memory = &mut machine.memory;
//! let ax = cpu.ax();
//! memory.write(0x200, &ax.to_le_bytes())?;
//! cpu.set_ax(ax.wrapping_add(1));
//! let saved = cpu.snapshot();
//! cpu.restore(&saved)?;
//! # Ok(())
//! # }
//! ```
#![forbid(unsafe_code)]

mod clock;
mod config;
mod cpu;
mod io;
mod memory;
mod runner;
pub mod runtime;

pub use clock::{timer_service, Clock, TimerDriver, TimerHandle};
pub use config::{ConfigurationError, CpuConfig, MachineConfig};
pub use cpu::CpuView;
pub use io::{Device, DeviceConnection, IoBus};
pub use memory::{Memory, MemoryError, MemoryView};
pub use relocade_wasm_runtime::MemoryAccess;
pub use runner::{yield_now, Runner};

#[cfg(feature = "worker")]
pub use relocade_wasm_runtime::worker;
pub use runtime::{
    real_mode_interrupt, Cpu, CpuContext, DispatchEntry, Error, Hook, HookResult, Result,
};
