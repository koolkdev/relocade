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
//! Pending and IRQ-suspended services survive dropping their run waiter. Dropping
//! the CPU abandons them innermost first without undoing completed effects. Restore
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
//!     let exit = cpu.step();
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
mod execution_interrupt;
mod hooks;
mod instruction_target;
mod io;
mod machine;
mod memory;
pub mod real_mode_interrupt;
mod step;
mod storage;

use crate::{CpuConfig, CpuStateAccess, IoBus, Memory};
use call::Call;
pub use context::CpuContext;
use dispatch::Dispatch;
pub use dispatch::DispatchEntry;
pub use error::{Error, Result};
pub use execution::ExitHandle;
use execution::{GuestCalls, SuspendedCall};
pub use execution_interrupt::ExecutionInterrupt;
pub use hooks::Hook;
pub use instruction_target::InstructionTarget;
use machine::Machine;
use memory::RuntimeMemory;
pub use storage::CpuStorage;

type Backend = relocade_wasm_runtime::Backend<IoBus>;

/// Executes one guest and owns its machine, registered services and suspended calls.
pub struct Cpu {
    pending: Option<Call>,
    suspended: Vec<SuspendedCall>,
    guest_calls: GuestCalls,
    machine: Machine,
    exit: ExitHandle,
    instruction_target: InstructionTarget,
    // Identifies the owning session once at run entry, never during hooks.
    pub(crate) executor: std::rc::Weak<crate::worker::Executor<ExecutionInterrupt>>,
}

impl Cpu {
    /// Creates CPU state and dispatch over existing guest memory.
    /// Guest bytes are preserved. Host services capture their application resources.
    /// Port and MMIO accesses use the supplied execution-local device bus.
    pub async fn new(memory: Memory, cpu: CpuConfig, io: IoBus) -> Result<Self> {
        CpuStorage::new()?.into_cpu(memory, cpu, io).await
    }

    async fn with_storage(
        memory: Memory,
        cpu: CpuConfig,
        io: IoBus,
        storage: CpuStorage,
    ) -> Result<Self> {
        let mut backend = Backend::with_state(io.clone())?;
        let bindings = RuntimeMemory::new(&mut backend, memory, cpu, storage)?;
        let host_imports = io::imports(&mut backend);
        let dispatch = Dispatch::new(&mut backend, &bindings, host_imports).await?;
        let instruction_target = InstructionTarget::new(&bindings);
        let machine = Machine {
            backend,
            bindings,
            io,
            dispatch,
        };
        Ok(Self {
            pending: None,
            suspended: Vec::new(),
            guest_calls: GuestCalls::default(),
            machine,
            exit: ExitHandle::new(),
            instruction_target,
            executor: std::rc::Weak::new(),
        })
    }

    /// Links a module to this runtime and publishes its callable exports.
    /// Each export has signature `() -> ExecutionExit` and owns its entry checks, CPU
    /// publication and continuation. Addresses are linear CS.base + EIP, before
    /// paging/A20. Installing an existing address replaces its function.
    /// Exports are independently replaceable; a batch does not tie their lifetimes.
    /// The last export supplied at a repeated address wins.
    ///
    /// The caller must maintain the module's execution assumptions and invalidate
    /// or replace captured code when guest bytes or mappings change.
    /// Module start functions must not enter guest dispatch or host hooks;
    /// execution belongs to `run`.
    ///
    /// # Panics
    /// Panics if a service is pending or parked. Complete or abandon it first.
    pub async fn install_module(
        &mut self,
        bytes: &[u8],
        entries: &[DispatchEntry<'_>],
    ) -> Result<()> {
        assert!(
            self.suspended.is_empty(),
            "host calls are parked; complete or abandon them before installing"
        );
        let machine = self.machine_mut();
        machine
            .dispatch
            .install_module(&mut machine.backend, &machine.bindings, bytes, entries)
            .await
    }

    /// Installs host and Wasm hooks in one batch. Each address is independently
    /// replaceable; the last hook at a repeated address wins. Replacing a host hook
    /// releases its old handler captures. Environment adapters supply all their hooks
    /// when replacing a service; a low-level overwrite changes only its address.
    ///
    /// The driver polls ordinary async handlers after guest Wasm returns. Ready
    /// awaits complete in one poll; pending calls survive waiter replacement.
    /// Hooks are intended for function and interrupt targets. Direct hooks at
    /// the protected successor of STI/MOV SS/POP SS remain unsupported; interpreter
    /// STEP executes those bytes. CALL/INT targets work normally. See
    /// <https://github.com/koolkdev/relocade/issues/709>.
    ///
    /// # Panics
    /// Panics if a host call is pending or parked. Complete or abandon it first.
    pub async fn install(&mut self, hooks: impl IntoIterator<Item = Hook>) -> Result<()> {
        assert!(
            self.suspended.is_empty(),
            "host calls are parked; complete or abandon them before installing"
        );
        let machine = self.machine_mut();
        machine
            .dispatch
            .install(&mut machine.backend, &machine.bindings, hooks)
            .await
    }

    /// Borrows the stopped machine. The context and its views must end before
    /// running or installing modules. Available while guest ISR execution is
    /// stopped with outer calls parked, but not while a host call is pending.
    ///
    /// # Panics
    /// Panics if a host call is pending. Complete it to regain access.
    pub fn context(&mut self) -> CpuContext<'_> {
        self.machine_mut().context()
    }

    fn machine_mut(&mut self) -> &mut Machine {
        assert!(
            self.pending.is_none(),
            "host call is pending; complete it before accessing the machine"
        );
        &mut self.machine
    }

    /// Retains live CPU state independently of the runner and its backend.
    /// Capture this once when registering a service, like memory and I/O handles.
    pub fn cpu_state(&self) -> CpuStateAccess {
        self.machine.bindings.cpu.clone()
    }

    /// Retains the execution interrupt signal, independently of CPU access.
    /// Requests cause `ExecutionExit::Interrupted` at a guest checkpoint; they do
    /// not wake a pending service. The embedding owns acknowledgement and work.
    pub fn interrupt_handle(&self) -> ExecutionInterrupt {
        self.machine.bindings.interrupt.clone()
    }

    /// Retains guest memory independently of this CPU and its execution borrow.
    /// Clones share guest bytes; dispatch storage remains private.
    pub fn memory(&self) -> Memory {
        self.machine.bindings.guest.clone()
    }
}
