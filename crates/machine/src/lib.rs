//! Machine configuration, live state access and asynchronous guest execution.
//!
//! [`Machine`] creates shared memory and CPU state before execution starts.
//! Each [`CpuHandle`] starts one execution session. Worker startup constructs the
//! [`Cpu`] backend over existing state before handing its [`Runner`] to the session.
//! The session attaches local devices; the machine retains state after the worker joins.
//!
//! [`CpuConfig`] describes initial CPU state. [`MachineConfig`] describes backing
//! size and the fixed physical map. [`CpuStateAccess`] and [`Memory`] retain live
//! resources independently of the runner. Host services capture them once and use
//! ordinary Rust awaits. [`CpuContext`] provides borrowed access for embeddings
//! and host services, using the machine's memory and I/O policy.
//!
//! ```
//! use relocade_machine::{CpuConfig, MachineConfig};
//! use relocade_x86::{Compiler, CpuState, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap};
//!
//! let target = Compiler::new(ExecutionProfile::Real16).target();
//! let cpu = CpuState { eip: 0x100, ..CpuState::real_mode() };
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
#![deny(unsafe_code)]

mod clock;
mod compiler;
mod config;
mod cpu;
mod io;
mod machine;
mod memory;
mod runner;
pub mod runtime;

#[cfg(test)]
extern crate self as relocade_machine;

pub use clock::{
    timer_service, Clock, ClockSource, GuestTimeDriver, HostTimerDriver, Instant, Sleep,
    TimerHandle, Timestamp,
};
pub use compiler::{BlockRequest, CompiledCode, CompilerClient, CompilerService};
pub use config::{ConfigurationError, CpuConfig, MachineConfig};
pub use cpu::{registers, CpuStateAccess, SavedRegisters};
pub use io::{
    Device, DeviceCallbacks, DeviceConnection, IoBus, IoContext, MmioBinding, MmioRead, MmioWrite,
    PortBinding, PortRange, PortRead, PortWrite,
};
pub use machine::{CpuHandle, Machine};
pub use memory::{FarPointer, Memory, MemoryBus, MemoryError, MemoryView};
pub use relocade_wasm_runtime::{worker, MemoryAccess};
pub use runner::{yield_now, ExecutionWorker, GuestTimeConfig, Runner};
pub use runtime::{
    real_mode_interrupt, Cpu, CpuContext, CpuStorage, DispatchEntry, Error, ExecutionInterrupt,
    ExitHandle, Hook, InstructionTarget, Result,
};
