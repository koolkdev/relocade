//! Execution of one guest, independent of the host Wasm engine.
//!
//! [`Cpu::new`] prepares an interpreter and dispatcher over supplied [`Memory`]
//! and initializes independently retained CPU storage. [`Cpu::context`] lends access to
//! the stopped CPU and mapped memory, both backed directly by Wasm storage.
//! CPU edits take effect immediately. Preparation is asynchronous
//! for native and web engines. [`Cpu::run`] awaits host operations
//! between synchronous Wasm calls.
//!
//! The driver owns the CPU instance directly and alternates guest calls with polling
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
mod guest;
mod hooks;
mod instance;
mod instruction_target;
mod io;
mod memory;
pub mod real_mode_interrupt;
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
use instance::CpuInstance;
pub use instruction_target::InstructionTarget;
use memory::RuntimeMemory;
use relocade_wasm_runtime::Module;
pub use storage::CpuStorage;

type Backend = relocade_wasm_runtime::Backend<IoBus>;

/// Executes one guest and owns its execution instance, registered services and suspended calls.
pub struct Cpu {
    pending: Option<Call>,
    suspended: Vec<SuspendedCall>,
    guest_calls: GuestCalls,
    instance: CpuInstance,
    exit: ExitHandle,
    instruction_target: InstructionTarget,
    halted: bool,
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
        storage: relocade_wasm_runtime::SharedMemory,
    ) -> Result<Self> {
        let mut backend = Backend::with_state(io.clone())?;
        let bindings = RuntimeMemory::new(&mut backend, memory, cpu, storage)?;
        let dispatch = Dispatch::new(&mut backend, &bindings).await?;
        let instruction_target = InstructionTarget::new(&bindings);
        let instance = CpuInstance {
            backend,
            bindings,
            io,
            dispatch,
        };
        Ok(Self {
            pending: None,
            suspended: Vec::new(),
            guest_calls: GuestCalls::default(),
            instance,
            exit: ExitHandle::new(),
            instruction_target,
            halted: false,
        })
    }

    /// Retains this CPU's fixed device bus. Connect its devices during setup;
    /// all host views and installed modules observe the same bus.
    pub fn io(&self) -> IoBus {
        self.instance.io.clone()
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
        let instance = self.instance_mut();
        instance
            .dispatch
            .install_module(&mut instance.backend, &instance.bindings, bytes, entries)
            .await
    }

    /// Instantiates an already compiled module against this CPU's imports and
    /// publishes its entries through the same dispatch path as [`Self::install_module`].
    /// The module can be compiled on another worker and reused across CPUs;
    /// each installation binds to the receiving CPU's state and memory.
    ///
    /// The caller maintains code and mapping validity. Replacement semantics and
    /// restrictions on module start functions match [`Self::install_module`].
    ///
    /// # Panics
    /// Panics if a service is pending or parked. Complete or abandon it first.
    pub fn install_compiled_module(
        &mut self,
        module: &Module,
        entries: &[DispatchEntry<'_>],
    ) -> Result<()> {
        assert!(
            self.suspended.is_empty(),
            "host calls are parked; complete or abandon them before installing"
        );
        let instance = self.instance_mut();
        instance.dispatch.install_compiled_module(
            &mut instance.backend,
            &instance.bindings,
            module,
            entries,
        )
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
        let instance = self.instance_mut();
        instance
            .dispatch
            .install(&mut instance.backend, &instance.bindings, hooks)
            .await
    }

    /// Borrows the stopped CPU and its mapped resources. The context and its views must end before
    /// running or installing modules. Available while guest ISR execution is
    /// stopped with outer calls parked, but not while a host call is pending.
    ///
    /// # Panics
    /// Panics if a host call is pending. Complete it to regain access.
    pub fn context(&mut self) -> CpuContext<'_> {
        self.instance_mut().context()
    }

    fn instance_mut(&mut self) -> &mut CpuInstance {
        assert!(
            self.pending.is_none(),
            "host call is pending; complete it before accessing the instance"
        );
        &mut self.instance
    }

    /// Retains live CPU state independently of the runner and its backend.
    /// Capture this once when registering a service, like memory and I/O handles.
    pub fn cpu_state(&self) -> CpuStateAccess {
        self.instance.bindings.cpu.clone()
    }

    /// Retains the execution interrupt signal, independently of CPU access.
    /// Requests cause `ExecutionExit::Interrupted` at a guest checkpoint; they do
    /// not wake a pending service. The embedding owns acknowledgement and work.
    pub fn interrupt_handle(&self) -> ExecutionInterrupt {
        self.instance.bindings.interrupt.clone()
    }

    /// Retains guest memory independently of this CPU and its execution borrow.
    /// Clones share guest bytes; dispatch storage remains private.
    pub fn memory(&self) -> Memory {
        self.instance.bindings.guest.clone()
    }
}
