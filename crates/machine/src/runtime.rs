//! Execution of one guest, independent of the host Wasm engine.
//!
//! [`Cpu::new`] prepares an interpreter and dispatcher over supplied [`Memory`]
//! and initializes independently retained CPU storage. [`Cpu::context`] lends access to
//! the stopped CPU and mapped memory, both backed directly by Wasm storage.
//! CPU edits take effect immediately. Preparation is asynchronous
//! for native and web engines. [`Cpu::run`] awaits host operations
//! between synchronous Wasm calls.
//!
//! The driver owns the machine directly and alternates guest calls with polling
//! ordinary Rust service futures. Handlers capture CPU state, memory, I/O and
//! application resources at registration. Access copies values without acquiring
//! the backend or binding a temporary view. Pending futures retain their locals
//! and may borrow their handler captures; completion drops the future first.
//!
//! A pending service survives dropping its run waiter. Dropping the CPU abandons
//! that future and destroys the backend without undoing completed effects. Restore
//! temporary CPU changes explicitly on normal completion, not from a destructor
//! that also runs on abandonment. Retained state and memory handles remain usable
//! independently of the runner's lifetime.
//!
//! ```
//! use relocade_machine::{Cpu, CpuConfig, MachineConfig, Memory};
//!
//! async fn example(config: MachineConfig, cpu_config: CpuConfig)
//!     -> Result<(), Box<dyn std::error::Error>>
//! {
//!     let memory = Memory::new(config)?;
//!     memory.view().write(0x100, &[0xb8, 42, 0, 0xeb, 0])?;
//!     let mut cpu = Cpu::new(memory, cpu_config, Default::default()).await?;
//!     let exit = cpu.run(100).await;
//!     let ax = cpu.context().cpu.ax();
//!     # let _ = (exit, ax);
//!     Ok(())
//! }
//! ```
#![deny(unsafe_code)]

#[allow(unsafe_code)]
mod call;
mod code;
mod context;
mod dispatch;
mod error;
mod execution;
mod hooks;
mod io;
mod machine;
mod memory;
pub mod real_mode_interrupt;
mod services;
mod stop;

use crate::{CpuConfig, CpuStateAccess, IoBus, Memory};
pub use context::CpuContext;
use dispatch::Dispatch;
pub use dispatch::DispatchEntry;
pub use error::{Error, Result};
use execution::Frame;
pub use hooks::Hook;
use machine::Machine;
use memory::RuntimeMemory;
use relocade_wasm_runtime::{WasmBackend, WasmCompiler};
use services::Services;
pub use stop::StopHandle;

type Backend = relocade_wasm_runtime::Backend<IoBus>;

/// Executes one guest and owns its machine, registered services and suspended calls.
pub struct Cpu {
    pending: Option<Frame>,
    machine: Machine,
    stop: StopHandle,
}

impl Cpu {
    /// Creates CPU state and dispatch over existing guest memory.
    /// Guest bytes are preserved. Host services capture their application resources.
    /// Port and MMIO accesses use the supplied execution-local device bus.
    pub async fn new(memory: Memory, cpu: CpuConfig, io: IoBus) -> Result<Self> {
        let mut backend = Backend::with_state(io.clone())?;
        let bindings = RuntimeMemory::new(&mut backend, memory, cpu)?;
        let host_imports = io::imports(&mut backend);
        let dispatch = Dispatch::new(&mut backend, &bindings, host_imports).await?;
        let machine = Machine {
            backend,
            bindings,
            io,
            dispatch,
            services: Services::default(),
        };
        Ok(Self {
            pending: None,
            machine,
            stop: StopHandle::new(),
        })
    }

    /// Links a module to this runtime and publishes its callable exports.
    /// Each export has signature `() -> ExecutionExit` and owns its entry checks, CPU
    /// publication and continuation. Addresses are linear CS.base + EIP, before
    /// paging/A20. Installing an existing address replaces its function.
    ///
    /// The caller must maintain the module's execution assumptions and invalidate
    /// or replace captured code when guest bytes or mappings change.
    /// Module start functions must not enter guest dispatch or host hooks;
    /// execution belongs to `run`.
    ///
    /// # Panics
    /// Panics if a service is suspended. Complete it before installing.
    pub async fn install_module(
        &mut self,
        bytes: &[u8],
        entries: &[DispatchEntry<'_>],
    ) -> Result<()> {
        let machine = self.machine_mut();
        let compiler = machine.backend.compiler();
        let module = compiler.compile(bytes).await?;
        let removed = machine
            .services
            .replaced_addresses(entries.iter().map(|entry| entry.address));
        machine
            .dispatch
            .install(
                &mut machine.backend,
                &machine.bindings,
                &module,
                entries,
                &removed,
            )
            .await?;
        machine.services.remove(&removed);
        Ok(())
    }

    /// Borrows the stopped machine. The context and its views must end before
    /// running or installing modules.
    ///
    /// # Panics
    /// Panics if a service is suspended. Complete it to regain access.
    pub fn context(&mut self) -> CpuContext<'_> {
        self.machine_mut().context()
    }

    fn machine_mut(&mut self) -> &mut Machine {
        assert!(
            self.pending.is_none(),
            "host service is suspended; complete it before accessing the machine"
        );
        &mut self.machine
    }

    /// Retains live CPU state independently of the runner and its backend.
    /// Capture this once when registering a service, like memory and I/O handles.
    pub fn cpu_state(&self) -> CpuStateAccess {
        self.machine.bindings.cpu.clone()
    }

    /// Retains a stop request independently of the CPU. Capture it in host
    /// services that terminate execution, or use it to wake and stop a pending run.
    pub fn stop_handle(&self) -> StopHandle {
        self.stop.clone()
    }

    /// Retains guest memory independently of this CPU and its execution borrow.
    /// Clones share guest bytes; dispatch storage remains private.
    pub fn memory(&self) -> Memory {
        self.machine.bindings.guest.clone()
    }
}
