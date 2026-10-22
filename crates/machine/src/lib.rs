//! Machines with shared guest memory and independently executable CPUs.
//!
//! [`Machine`] owns the guest memory shared by its CPUs.
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
mod storage;
#[cfg(feature = "worker")]
pub use runtime::worker;

pub use config::{ConfigurationError, CpuConfig, MachineConfig};
pub use cpu::{Cpu, CpuContext, CpuView};
pub use machine::Machine;
pub use memory::{Memory, MemoryError, MemoryView};
pub use runtime::{
    real_mode_interrupt, CpuBindings, DispatchEntry, Error, Hook, HostCall, Result, RuntimeBackend,
    RuntimeState,
};
pub use storage::MemoryStorage;

/// Transfers to the private CPU context and shared guest storage. CPU offsets
/// are relative to `CpuState`; writes must update live storage immediately.
pub trait CpuStorage: MemoryStorage {
    fn read_cpu(&self, offset: u32, bytes: &mut [u8]);
    fn write_cpu(&mut self, offset: u32, bytes: &[u8]);
}
