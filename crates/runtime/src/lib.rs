//! Execution of one guest, independent of the host Wasm engine.
//!
//! [`Runtime::new`] prepares an interpreter and dispatcher and initializes
//! backend storage from [`MachineConfig`]. [`Runtime::machine`] lends access to
//! the stopped CPU and mapped memory, both backed directly by Wasm storage.
//! CPU edits take effect immediately. Preparation is asynchronous
//! for native and web engines. [`Runtime::run_async`] can await host operations
//! between synchronous Wasm calls.
//!
//! ```
//! use relocade_machine::MachineConfig;
//! use relocade_runtime::{Runtime, RuntimeBackend, RuntimeState};
//!
//! async fn example<B: RuntimeBackend<State = RuntimeState>>(
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
pub mod compiler_worker;
mod dispatch;
mod error;
mod execution;
mod hooks;
mod memory;
pub mod real_mode_interrupt;
mod state;
mod storage;
pub mod wasm;

pub use backend::RuntimeBackend;
pub use callback::{with_host_call, HostCall};
pub use dispatch::DispatchEntry;
use dispatch::DispatchTable;
pub use error::{Error, Result};
pub use hooks::Hook;
pub use memory::SharedMemory;
use relocade_machine::{Machine, MachineConfig};
use relocade_x86::{abi, Compiler, EntryTable, ExecutionContext};
pub use state::RuntimeState;
use std::{
    ops::{Deref, DerefMut},
    sync::Arc,
};
use storage::Storage;
use wasm::Import;

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
    host_call_incomplete: bool,
}

impl<T: 'static, B: RuntimeBackend<State = RuntimeState<T>>> Runtime<B> {
    /// Prepares code and allocates zeroed backing with the configured CPU and map.
    /// The runtime retains the backend and its live storage for subsequent calls.
    /// Port and MMIO reads currently return all ones; their writes are ignored.
    pub async fn new(mut backend: B, config: MachineConfig) -> Result<Self> {
        let target = config.execution_target();
        let compiler = Compiler::new(target.profile).with_cpu_model(target.cpu_model);
        let dispatcher = compiler.compile_dispatcher()?;
        let dispatcher_module = backend.compile(&dispatcher.bytes).await?;

        let context = backend.create_memory(relocade_compiler::abi::Memory {
            maximum: Some(1),
            ..abi::EXECUTION_CONTEXT
        })?;
        backend.write_memory(
            &context,
            0,
            &ExecutionContext {
                cpu: *config.cpu(),
                cpu_model: target.cpu_model,
                ..ExecutionContext::default()
            }
            .to_bytes(),
        );
        let guest_pages = config.memory_size().div_ceil(65536).max(1) as u32;
        let guest = backend.create_memory(relocade_compiler::abi::Memory {
            minimum: guest_pages,
            ..abi::GUEST
        })?;
        let mapping = backend.create_memory(relocade_compiler::abi::Memory {
            maximum: Some(1),
            ..abi::PHYSICAL_MAP
        })?;
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
            host_call_incomplete: false,
        };
        let bytes = relocade_interpreter::module(target.profile);
        let backend = &runtime.storage.access;
        let module = if let Some(cache) = backend.module_cache() {
            cache
                .get_or_compile(bytes, |bytes| backend.compile(bytes))
                .await?
        } else {
            backend.compile(bytes).await?
        };
        let instance = runtime.instantiate_module(&module, &[]).await?;
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
        let module = self.storage.access.compile(bytes).await?;
        self.instantiate_module(&module, additional).await
    }

    async fn instantiate_module(
        &mut self,
        module: &B::Module,
        additional: &[Import<'_, B>],
    ) -> Result<B::Instance> {
        let backend = &mut self.storage.access;
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
        backend.instantiate(module, &imports).await
    }

    /// Borrows the stopped machine. CPU and memory writes take effect immediately.
    pub fn machine(&mut self) -> Machine<'_> {
        Machine::new(&self.config, &mut self.storage)
    }

    /// Shares guest memory with a reader that can outlive this runtime. Native
    /// readers can move to another thread; web readers can send their backing
    /// WebAssembly.Memory to a worker. CPU and dispatch storage remain private.
    pub fn shared_memory(&self) -> SharedMemory<B::SharedMemory> {
        SharedMemory::new(
            &self.config,
            self.storage
                .access
                .shared_memory(&self.storage.guest)
                .expect("guest backing is shared"),
        )
    }

    /// Borrows the environment state between guest calls. Drop the borrow before running.
    pub fn state(&self) -> impl Deref<Target = T> + '_ {
        state::Data(self.storage.access.state())
    }

    pub fn state_mut(&mut self) -> impl DerefMut<Target = T> + '_ {
        state::Data(self.storage.access.state_mut())
    }
}
