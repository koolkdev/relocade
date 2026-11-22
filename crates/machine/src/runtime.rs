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
//! application resources at registration. CPU state and RAM access use shared
//! storage directly. Host device access briefly borrows the backend; guest
//! callbacks use the current engine scope. Pending futures retain their locals
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
mod error;
mod execution;
mod execution_interrupt;
mod guest;
mod hooks;
mod instance;
mod memory;
pub mod real_mode_interrupt;
mod storage;
mod wasm;

use crate::{CpuConfig, CpuStateAccess, IoBus, Memory};
use call::Call;
pub use context::CpuContext;
pub use error::{Error, Result};
pub(crate) use execution::CpuStop;
pub use execution::ExitHandle;
use execution::{GuestCalls, SuspendedCall};
pub use execution_interrupt::ExecutionInterrupt;
pub use hooks::Hook;
pub(crate) use instance::CodeWake;
pub use instance::DispatchEntry;
use instance::{CpuInstance, Installation};
use memory::RuntimeMemory;
use relocade_wasm_runtime::{Module, WasmRuntime};
pub use storage::CpuStorage;
pub(crate) use wasm::WasmRuntimeHandle;

/// Executes one guest and owns its execution instance, registered services and suspended calls.
pub struct Cpu {
    pub(crate) execution: Option<crate::machine::execution::Member>,
    pending: Option<Call>,
    suspended: Vec<SuspendedCall>,
    guest_calls: GuestCalls,
    instance: CpuInstance,
    exit: ExitHandle,
    halted: bool,
}

impl Cpu {
    /// Creates CPU state and dispatch over existing guest memory.
    /// Returns [`crate::ConfigurationError::CpuAlreadyAttached`] if that memory
    /// already has a CPU. Dropping a standalone CPU releases its attachment.
    /// Guest bytes are preserved. Host services capture their application resources.
    /// Port and MMIO accesses use the supplied execution-local device bus.
    ///
    /// # Panics
    /// Panics if this bus was already attached to a CPU, including one that was
    /// dropped or whose initialization failed. Each CPU needs its own bus.
    pub async fn new(memory: Memory, cpu: CpuConfig, io: IoBus) -> Result<Self> {
        CpuStorage::new()?.into_cpu(memory, cpu, io).await
    }

    /// Creates a protected Flat32 interpreter over flat guest memory.
    /// Drive CPUs sharing this backing cooperatively for unaligned locked operands.
    /// Inspect its registers and memory through [`Self::cpu_state`] and [`Self::memory`].
    /// Dispatch and host hook keys are linear EIP addresses. Instructions use the
    /// loaded segment caches in `cpu`. Descriptor queries and reloads see empty tables.
    pub async fn new_without_io(memory: Memory, cpu: CpuConfig) -> Result<Self> {
        CpuStorage::new()?.attach(memory, cpu, None).await
    }

    async fn with_storage(
        binding: crate::memory::CpuBinding,
        io: Option<IoBus>,
        storage: relocade_wasm_runtime::SharedMemory,
    ) -> Result<Self> {
        let instance = CpuInstance::new(binding, io, storage).await?;
        Ok(Self {
            execution: None,
            pending: None,
            suspended: Vec::new(),
            guest_calls: GuestCalls::default(),
            instance,
            exit: ExitHandle::new(),
            halted: false,
        })
    }

    /// Returns an owned handle to a PC CPU's device bus.
    ///
    /// The handle retains its devices and runtime independently of this CPU and
    /// supports synchronous port and MMIO access at stopped host boundaries.
    /// Connect devices during setup; all host views and installed modules observe
    /// the same bus. Capture it when registering a service, like [`Self::cpu_state`]:
    ///
    /// ```
    /// use relocade_machine::{Cpu, Hook};
    ///
    /// fn status_service(cpu: &Cpu) -> Hook {
    ///     let state = cpu.cpu_state();
    ///     let io = cpu.io();
    ///     Hook::host(0xf000_0100, async move || {
    ///         io.write_port(0x3d4, 1, 0x0a);
    ///         state.set_al(io.read_port(0x3d5, 1) as u8);
    ///     })
    /// }
    /// ```
    ///
    /// # Panics
    /// Panics for a CPU constructed with [`Self::new_without_io`].
    pub fn io(&self) -> IoBus {
        self.instance.guest.environment.pc_bus().clone()
    }

    /// Installed compilations whose decoded bytes cover this linear address.
    /// Several modules can cover the same bytes; coverage does not make an
    /// interior address a callable entry. Each installation stays here until retirement.
    pub fn compiled_modules_at(&self, address: u32) -> impl Iterator<Item = &crate::CompiledCode> {
        self.instance.code.compiled_modules_at(address)
    }

    /// Retires this CPU's compiled modules whose decoded source overlaps `range`.
    /// Addresses are linear, before A20; the exclusive end can be `1 << 32`.
    /// Empty ranges have no effect. Retiring a linked module also withdraws its
    /// other entries, because direct calls within that version remain immutable.
    /// Incoming call links return to resolution. Entries owned by unaffected
    /// replacements or hooks survive.
    ///
    /// Use this at a stopped boundary to withdraw code explicitly. Managed writes
    /// to stopped guest memory trigger retirement before the next dispatch.
    ///
    /// # Panics
    /// Panics if a service is pending or parked. Complete or abandon it first.
    pub fn invalidate_code(&mut self, range: std::ops::Range<u64>) -> Result<()> {
        if range.end > 1_u64 << 32 {
            return Err(crate::MemoryError::AddressRange.into());
        }
        self.stopped_code().invalidate_code(range)
    }

    pub(crate) fn install_guest_code(&mut self, code: &crate::CompiledCode) -> Result<()> {
        self.stopped_code()
            .install_guest_code(code, Installation::Explicit)
    }

    /// Compiles a caller-supplied Wasm module and publishes its callable exports on this CPU.
    /// Each export has signature `() -> ExecutionExit` and owns its entry checks, CPU
    /// publication and continuation. Keys are packed real-mode CS:IP or flat EIP. Installing an
    /// existing key replaces its function.
    /// Exports are independently replaceable; a batch does not tie their lifetimes.
    /// The last export supplied at a repeated key wins.
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
        self.stopped_code().install_wasm_bytes(bytes, entries).await
    }

    /// Instantiates a caller-supplied Wasm module against this CPU's imports and
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
        self.stopped_code().install_wasm_module(module, entries)
    }

    /// Installs host and Wasm hooks in one batch. Each CS:IP entry is independently
    /// replaceable; the last hook at a repeated key wins. Replacing a host hook
    /// releases its old handler captures. Environment adapters supply all their hooks
    /// when replacing a service; a low-level overwrite changes only its entry.
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
        self.stopped_code().install(hooks).await
    }

    /// Borrows the stopped PC CPU, its device bus and mapped memory. The context and its views must end before
    /// running or installing modules. Available while guest ISR execution is
    /// stopped with outer calls parked, but not while a host call is pending.
    ///
    /// # Panics
    /// Panics if a host call is pending or the CPU has no PC bus.
    pub fn context(&mut self) -> CpuContext<'_> {
        self.instance_mut().context()
    }

    fn stopped_code(&mut self) -> &mut instance::CodeCache {
        assert!(
            self.suspended.is_empty(),
            "complete or abandon parked host calls before changing installed code"
        );
        &mut self.instance_mut().code
    }

    fn instance_mut(&mut self) -> &mut CpuInstance {
        assert!(
            self.pending.is_none(),
            "host call is pending; complete it before accessing the instance"
        );
        &mut self.instance
    }

    /// Retains live CPU state independently of the runner and its Wasm runtime.
    /// Capture this once when registering a service, like memory and I/O handles.
    pub fn cpu_state(&self) -> CpuStateAccess {
        self.instance.guest.bindings.cpu.clone()
    }

    /// Retains the execution interrupt signal, independently of CPU access.
    /// Requests cause `ExecutionExit::Interrupted` at a guest checkpoint; they do
    /// not wake a pending service. The embedding owns acknowledgement and work.
    pub fn interrupt_handle(&self) -> ExecutionInterrupt {
        self.instance.guest.bindings.interrupt.clone()
    }

    /// Retains guest memory independently of this CPU and its execution borrow.
    /// Clones share guest bytes; dispatch storage remains private.
    pub fn memory(&self) -> Memory {
        self.instance.guest.bindings.guest.clone()
    }
}
