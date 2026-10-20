//! Execution of one guest, independent of the host Wasm engine.
//!
//! [`Runtime::new`] prepares an interpreter and dispatcher and initializes
//! backend storage from [`MachineConfig`]. [`Runtime::machine`] lends access to
//! the stopped CPU and mapped memory, both backed directly by Wasm storage.
//! CPU edits take effect immediately. Preparation is asynchronous
//! for native and web engines. [`Runtime::run_async`] can await host operations
//! between synchronous Wasm calls.
//!
//! With a compiler worker installed, `run_async` queues hot interpreter entries
//! and keeps executing while the worker discovers connected blocks and compiles
//! a module. Ready results are published between cooperative execution slices.
//! [`JitConfig`] controls promotion, compilation and cache bounds; synchronous
//! [`Runtime::run`] exposes requests for callers using [`Runtime::queue_hot_request`].
//! See the [worker contract](https://github.com/koolkdev/relocade/blob/main/crates/runtime/docs/worker-execution.md)
//! for browser setup.
//!
//! Managed groups capture readable code with [`Runtime::prepare_group`] and
//! publish it with [`Runtime::install_group`]. Captures include their execution
//! identity; publication rechecks consumed instruction bytes and fixed entries.
//! Guest stores, stopped memory edits and hook memory edits retire every entry
//! of affected groups before changing the backing. Page-map watches cover RAM
//! aliases, including writable aliases of ROM code. Ordinary RAM accesses retain
//! their existing direct-access predicate.
//!
//! ```no_run
//! # use relocade_runtime::{BlockRequest, Runtime, RuntimeBackend, RuntimeState};
//! # async fn example<B: RuntimeBackend<State = RuntimeState>>(runtime: &mut Runtime<B>) -> relocade_runtime::Result<()> {
//! let entries = [BlockRequest { start_eip: 0x1000, instruction_limit: 32 }];
//! if let Some(group) = runtime.prepare_group(&entries)? {
//!     let installed = runtime.install_group(group).await?;
//!     // None means intervening code or entry changes made this capture stale.
//!     # let _ = installed;
//! }
//! # Ok(())
//! # }
//! ```
//!
//! [`Runtime::capture_group`] and [`Runtime::capture_connected_group`] only copy
//! a bounded readable code image. The resulting [`CodeSnapshot`] can move to a
//! compiler thread: `snapshot.compile()` performs discovery and Wasm lowering,
//! without borrowing the runtime. A backend compiles `prepared.wasm()` and returns
//! [`CompiledGroup`]; [`Runtime::publish_group`] then binds the guest's private
//! imports and validates the consumed code before exposing any entries.
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
mod code;
pub mod compilation;
mod dispatch;
mod error;
mod execution;
mod hooks;
mod hotness;
mod jit;
mod memory;
pub mod real_mode_interrupt;
mod state;
mod storage;
pub mod wasm;

pub use backend::RuntimeBackend;
pub use callback::{with_host_call, HostCall};
pub use code::{BlockRequest, CodeSnapshot, CompiledGroup, GroupId, PreparedGroup};
pub use dispatch::DispatchEntry;
use dispatch::DispatchTable;
pub use error::{Error, Result};
pub use hooks::Hook;
pub use jit::JitConfig;
pub use memory::SharedMemory;
use relocade_machine::{Machine, MachineConfig};
pub use relocade_x86::HotEntry;
use relocade_x86::{abi, Compiler, EntryTable, ExecutionContext};
pub use state::RuntimeState;
use std::{
    ops::{Deref, DerefMut},
    sync::Arc,
};
use storage::Storage;
use wasm::{Import, WasmMemoryAccess};

/// One running guest and its engine resources. CPU and memory access belongs to
/// the machine; this owner prepares code and controls entry into execution.
pub struct Runtime<B: RuntimeBackend> {
    config: Arc<MachineConfig>,
    storage: Storage<B>,
    dispatch: B::Function,
    interpret: B::Function,
    mapping: B::Memory,
    hotness: B::Memory,
    compilation: compilation::CompilationQueue<B::Module>,
    jit: jit::JitState,
    execution_slice: std::num::NonZeroU64,
    dispatch_table: DispatchTable<B>,
    host_imports: Vec<Import<'static, B>>,
    host_call_incomplete: bool,
}

impl<T: 'static, B: RuntimeBackend<State = RuntimeState<T>>> Runtime<B> {
    /// Prepares code and allocates zeroed backing with the configured CPU and map.
    /// The runtime retains the backend and its live storage for subsequent calls.
    /// Port and MMIO reads currently return all ones; their writes are ignored.
    pub async fn new(mut backend: B, config: MachineConfig) -> Result<Self> {
        let compilation = compilation::CompilationQueue::new(backend.create_compiler()?);
        let target = config.execution_target();
        let compiler = Compiler::new(target.profile)
            .with_cpu_model(target.cpu_model)
            .with_hotness_tracking();
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
        let hotness = backend.create_memory(relocade_compiler::abi::Memory {
            maximum: Some(1),
            ..abi::HOTNESS
        })?;
        let dispatch_table = DispatchTable::new(&mut backend)?;
        let code = code::CodeCache::new(
            config.memory_map().clone(),
            Arc::clone(&dispatch_table.entries),
            dispatch_table.memory.clone(),
            mapping.clone(),
        );
        let code_writes = code.clone();
        let code_backing = guest.clone();
        let host_imports = vec![
            backend.bind_host(abi::READ_PORT, Box::new(|_, _, _| -1)),
            backend.bind_host(abi::WRITE_PORT, Box::new(|_, _, _, _| {})),
            backend.bind_host(abi::READ_MMIO, Box::new(|_, _, _| -1)),
            backend.bind_host(abi::WRITE_MMIO, Box::new(|_, _, _, _| {})),
            backend.bind_host(
                abi::WRITE_CODE,
                Box::new(move |mut caller, offset, bytes, value| {
                    code_writes.before_write(&mut caller, offset as u32, bytes as usize);
                    caller.write_memory(
                        &code_backing,
                        offset as u32,
                        &value.to_le_bytes()[..bytes as usize],
                    );
                }),
            ),
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
                code: Some(code),
            },
            dispatch,
            interpret,
            mapping,
            hotness,
            compilation,
            jit: jit::JitState::default(),
            execution_slice: std::num::NonZeroU64::new(65_536).unwrap(),
            dispatch_table,
            host_imports,
            host_call_incomplete: false,
        };
        let bytes = relocade_interpreter::runtime_module(target.profile);
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
        let profile = backend.function(&instance, abi::PROFILE)?;
        backend.set_table_function(
            &runtime.dispatch_table.functions,
            EntryTable::PROFILER_SLOT,
            &profile,
        )?;
        Ok(runtime)
    }

    /// Links a module to this runtime and publishes its callable exports.
    /// Each export has signature `() -> i64` and owns its entry checks, CPU
    /// publication and continuation. Addresses are linear CS.base + EIP, before
    /// paging/A20. Installing an existing address replaces its function.
    ///
    /// The caller must maintain the module's execution assumptions and invalidate
    /// or replace captured code when guest bytes or mappings change. These fixed
    /// entries are preserved as boundaries by managed compilation. Use
    /// `prepare_group`/`install_group` for runtime-owned snapshot validity.
    pub async fn install_module(
        &mut self,
        bytes: &[u8],
        entries: &[DispatchEntry<'_>],
    ) -> Result<()> {
        let instance = self.instantiate(bytes, &[]).await?;
        self.publish_fixed(&instance, entries)
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
            backend.bind(abi::HOTNESS, &self.hotness),
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
