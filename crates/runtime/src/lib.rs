//! Execution of one guest, independent of the host Wasm engine.
//!
//! [`Runtime::new`] prepares an interpreter and dispatcher and initializes private
//! backend storage from [`MachineConfig`]. [`Runtime::machine`] lends access to
//! the stopped CPU and mapped memory, both backed directly by Wasm storage.
//! CPU edits take effect immediately. Preparation is asynchronous
//! for native and web engines; guest execution is synchronous.
//!
//! ```
//! use wasm86_machine::MachineConfig;
//! use wasm86_runtime::{Runtime, RuntimeBackend};
//!
//! async fn example<B: RuntimeBackend>(
//!     backend: B,
//!     config: MachineConfig,
//! ) -> Result<(), Box<dyn std::error::Error>> {
//!     let mut runtime = Runtime::new(backend, config).await?;
//!     runtime.machine().memory().write(0x100, &[0xb8, 42, 0, 0xeb, 0])?;
//!     let exit = runtime.run(100)?;
//!     let ax = runtime.machine().cpu().ax();
//!     # let _ = (exit, ax);
//!     Ok(())
//! }
//! ```
#![forbid(unsafe_code)]

mod backend;
mod callback;
mod dispatch;
mod error;
mod hooks;
mod storage;
pub mod wasm;

pub use backend::RuntimeBackend;
pub use callback::{with_host_call, HostCall};
pub use dispatch::DispatchEntry;
use dispatch::DispatchTable;
pub use error::{Error, Result};
pub use hooks::Hook;
use std::{
    ops::{Deref, DerefMut},
    sync::Arc,
};
use storage::Storage;
use wasm::Import;
use wasm86_machine::{Machine, MachineConfig};
use wasm86_x86::{abi, Compiler, EntryTable, ExecutionContext, ExecutionExit};

/// One running guest and its engine resources. CPU and memory access belongs to
/// the machine; this owner prepares code and controls entry into execution.
pub struct Runtime<B: RuntimeBackend> {
    config: Arc<MachineConfig>,
    storage: Storage<B>,
    dispatch: B::Function,
    interpret: B::Function,
    mapping: B::Memory,
    dispatch_table: DispatchTable<B>,
    host_imports: Vec<Import<'static, B>>,
}

impl<B: RuntimeBackend> Runtime<B> {
    /// Prepares code and allocates zeroed backing with the configured CPU and map.
    /// The runtime retains the backend and its live storage for subsequent calls.
    /// Port and MMIO reads currently return all ones; their writes are ignored.
    pub async fn new(mut backend: B, config: MachineConfig) -> Result<Self> {
        let target = config.execution_target();
        let compiler = Compiler::new(target.profile).with_cpu_model(target.cpu_model);
        let interpreter = compiler.compile_interpreter()?;
        let dispatcher = compiler.compile_dispatcher()?;
        let dispatcher_module = backend.compile(&dispatcher.bytes).await?;

        let context = backend.create_memory(1, Some(1))?;
        backend.write_memory(
            &context,
            0,
            &ExecutionContext {
                cpu: *config.cpu(),
                ..ExecutionContext::default()
            }
            .to_bytes(),
        );
        let guest_pages = config.memory_size().div_ceil(65536).max(1) as u32;
        let guest = backend.create_memory(guest_pages, None)?;
        let mapping = backend.create_memory(1, Some(1))?;
        backend.write_memory(&mapping, 0, &config.memory_map().to_bytes());
        let dispatch_table = DispatchTable::new(&mut backend)?;
        let host_imports = vec![
            backend.bind_host(abi::READ_PORT, Box::new(|_, _, _| -1)),
            backend.bind_host(abi::WRITE_PORT, Box::new(|_, _, _, _| {})),
            backend.bind_host(abi::READ_MMIO, Box::new(|_, _, _| -1)),
            backend.bind_host(abi::WRITE_MMIO, Box::new(|_, _, _, _| {})),
        ];
        let imports = [
            backend.bind(abi::EXECUTION_CONTEXT, &context),
            backend.bind(abi::GUEST, &guest),
            backend.bind(abi::PHYSICAL_MAP, &mapping),
            backend.bind(abi::EXECUTION_ENTRIES, &dispatch_table.memory),
            backend.bind(abi::EXECUTION_FUNCTIONS, &dispatch_table.functions),
        ];
        let dispatcher_instance = backend.instantiate(&dispatcher_module, &imports).await?;
        let dispatch = backend.function(&dispatcher_instance, abi::DISPATCH)?;
        let interpret = backend.function(&dispatcher_instance, abi::INTERPRET)?;
        let mut runtime = Self {
            config: Arc::new(config),
            storage: Storage {
                access: backend,
                context,
                guest,
            },
            dispatch,
            interpret,
            mapping,
            dispatch_table,
            host_imports,
        };
        let instance = runtime.instantiate(&interpreter.bytes, &[]).await?;
        let backend = &mut runtime.storage.access;
        let function = backend.function(&instance, abi::RUN)?;
        backend.set_table_function(
            &runtime.dispatch_table.functions,
            EntryTable::INTERPRETER_SLOT,
            &function,
        )?;
        Ok(runtime)
    }

    /// Links a module to this runtime and publishes its callable exports.
    /// Each export has signature `() -> i64` and owns its entry checks, CPU
    /// publication and continuation. Addresses are linear CS.base + EIP, before
    /// paging/A20. Installing an existing address replaces its function.
    ///
    /// The caller must maintain the module's execution assumptions and invalidate
    /// or replace captured code when guest bytes or mappings change.
    pub async fn install_module(
        &mut self,
        bytes: &[u8],
        entries: &[DispatchEntry<'_>],
    ) -> Result<()> {
        let instance = self.instantiate(bytes, &[]).await?;
        self.dispatch_table
            .install(&mut self.storage.access, &instance, entries)
    }

    async fn instantiate(
        &mut self,
        bytes: &[u8],
        additional: &[Import<'_, B>],
    ) -> Result<B::Instance> {
        let backend = &mut self.storage.access;
        let module = backend.compile(bytes).await?;
        let mut imports = vec![
            backend.bind(abi::EXECUTION_CONTEXT, &self.storage.context),
            backend.bind(abi::GUEST, &self.storage.guest),
            backend.bind(abi::PHYSICAL_MAP, &self.mapping),
            backend.bind(abi::EXECUTION_ENTRIES, &self.dispatch_table.memory),
            backend.bind(abi::EXECUTION_FUNCTIONS, &self.dispatch_table.functions),
            backend.bind(abi::DISPATCH, &self.dispatch),
            backend.bind(abi::INTERPRET, &self.interpret),
        ];
        imports.extend(self.host_imports.iter().cloned());
        imports.extend(additional.iter().cloned());
        backend.instantiate(&module, &imports).await
    }

    /// Borrows the stopped machine. CPU and memory writes take effect immediately.
    pub fn machine(&mut self) -> Machine<'_> {
        Machine::new(&self.config, &mut self.storage)
    }

    /// Borrows the environment state between guest calls. Drop the borrow before running.
    pub fn state(&self) -> impl Deref<Target = B::State> + '_ {
        self.storage.access.state()
    }

    pub fn state_mut(&mut self) -> impl DerefMut<Target = B::State> + '_ {
        self.storage.access.state_mut()
    }

    /// Runs until the budget is consumed at a block boundary, a guest exit
    /// occurs, or a host function requests a return. Whole blocks can exceed the budget; straight-line code and REP
    /// have no fixed overshoot or elapsed-time bound. Zero does not enter Wasm.
    /// The absolute target saturates at u64::MAX, which is practically unbounded.
    pub fn run(&mut self, instruction_budget: u64) -> Result<ExecutionExit> {
        if instruction_budget == 0 {
            return Ok(ExecutionExit::BudgetExhausted);
        }
        let target = self
            .machine()
            .cpu()
            .instruction_count()
            .saturating_add(instruction_budget);
        self.storage.set_instruction_target(target);
        let word = self.storage.access.call(&self.dispatch)? as u64;
        ExecutionExit::from_bits(word).ok_or(Error::InvalidExit(word))
    }
}
