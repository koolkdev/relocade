//! Machines with shared guest memory and independently executable CPUs.
//!
//! [`Machine`] owns the guest memory, CPU membership and runtime code management.
//! Each [`Cpu`] retains its machine and owns its backend, live registers, dispatch
//! bindings and pending host calls. Running a CPU does not borrow the machine.
//! [`CpuContext`] and [`CpuView`] lend access to a stopped CPU's live storage.
//!
//! ```
//! use relocade_machine::{CpuConfig, Machine, MachineConfig, RuntimeBackend, RuntimeState};
//!
//! async fn example<B: RuntimeBackend<State = RuntimeState>>(
//!     mut backend: B, config: MachineConfig, cpu: CpuConfig,
//! ) -> Result<(), Box<dyn std::error::Error>> {
//!     let machine = Machine::new(&mut backend, config)?;
//!     machine.memory().view().write(0x100, &[0xb8, 42, 0, 0xeb, 0])?;
//!     let mut cpu = machine.create_cpu(backend, cpu).await?;
//!     let exit = cpu.run(100)?;
//!     let ax = cpu.context().cpu().ax();
//!     # let _ = (exit, ax);
//!     Ok(())
//! }
//! ```
#![forbid(unsafe_code)]

mod config;
mod cpu;
mod machine;
mod memory;
pub mod runtime;
#[cfg(feature = "worker")]
pub use runtime::worker;

pub use config::{ConfigurationError, CpuConfig, MachineConfig};
pub use cpu::{Cpu, CpuContext, CpuId, CpuView};
pub use machine::Machine;
pub use memory::{Memory, MemoryError, MemoryView};
pub use runtime::{
    real_mode_interrupt, CpuBindings, DispatchEntry, Error, Hook, HostCall, Result, RuntimeBackend,
    RuntimeState,
};

#[cfg(test)]
mod tests;
