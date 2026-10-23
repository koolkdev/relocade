//! Native and browser Wasm compilation, linking and execution.
//!
//! [`Backend`] uses Wasmtime on native targets and the browser WebAssembly APIs
//! on wasm32. Compiled code may be reused across compatible backends. Instances
//! bind to one backend's memories, tables and functions.
//!
//! Portable Rust codecs and browser adapters are supplied by
//! `relocade-wasm-bindings`. That crate owns direct calls, callback lifetimes and
//! generated return adapters. Native bindings use Wasmtime typed functions.
//! wasm32 applications must link with `--growable-table` and
//! `--export=__stack_pointer`, as configured by this workspace. Callbacks retain
//! their Rust owners through Wasm externrefs until their last engine reference
//! is collected. Wasm traps and invalid ABI values are fatal programming errors.
//!
//! ```
//! use relocade_wasm_runtime::{Backend, WasmBackend, WasmCompiler};
//!
//! # async fn example() -> relocade_wasm_runtime::Result<()> {
//! let mut backend = Backend::new()?;
//! let compiler = backend.compiler();
//! let module = compiler.compile(b"\0asm\x01\0\0\0").await?;
//! let instance = backend.instantiate(&module, &[]).await?;
//! # Ok(())
//! # }
//! ```
#![deny(unsafe_code)]

use relocade_compiler::abi::{Function as FunctionDeclaration, Memory as MemoryDeclaration, Value};
use std::future::Future;
use std::ops::{Deref, DerefMut};

mod error;
pub use error::{Error, Result};
#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(not(target_arch = "wasm32"))]
pub use native::{Backend, Compiler, Context, Execution, Memory, Module, SharedMemory};
#[cfg(target_arch = "wasm32")]
pub use web::{Backend, Compiler, Context, Execution, Memory, Module, SharedMemory};

mod calls;
mod host;
mod imports;
mod memory;
mod module_cache;
pub use calls::{CallSignature, ExportFunction, Function};
pub use host::{HostFunction, HostSignature};
pub use imports::{Import, ImportBinding, ImportValue};
pub use memory::{
    MemoryAccess, MemoryStorage, SharedMemoryStorage, WasmMemoryAccess, WasmSharedMemory,
};
pub use module_cache::ModuleCache;

/// Compiles Wasm bytes without borrowing a backend or retaining its host state.
/// The handle can move to another thread; create and await its futures there.
/// Futures and compiled modules need not be `Send` because browser engine
/// objects belong to the JavaScript thread that creates them.
pub trait WasmCompiler: Clone + Send + 'static {
    type Module: Clone + 'static;

    /// Compiles on the calling thread without creating a guest instance.
    fn compile(&self, bytes: &[u8]) -> impl Future<Output = Result<Self::Module>>;
}

/// Access to host state and live Wasm memory. Callback views cannot outlive
/// their callback. An execution owner can retain access between calls and
/// across awaits; its borrows must end before it enters Wasm again.
pub trait WasmContext: WasmMemoryAccess {
    type State;

    fn state(&self) -> &Self::State;
    fn state_mut(&mut self) -> &mut Self::State;

    /// Lends disjoint memory and host state during one stopped phase.
    fn memory_and_state<'a>(
        &'a mut self,
        memory: &'a Self::Memory,
    ) -> (MemoryAccess<'a>, &'a mut Self::State);
}

impl<C: WasmContext + ?Sized> WasmContext for &mut C {
    type State = C::State;

    fn state(&self) -> &Self::State {
        (**self).state()
    }

    fn state_mut(&mut self) -> &mut Self::State {
        (**self).state_mut()
    }

    fn memory_and_state<'a>(
        &'a mut self,
        memory: &'a Self::Memory,
    ) -> (MemoryAccess<'a>, &'a mut Self::State) {
        (**self).memory_and_state(memory)
    }
}

/// One Wasm resource owner. Native memory handles are Send + Sync; browser
/// resources stay on their JavaScript thread. Preparation futures and environment
/// state need not be Send. Memory is Wasm32 with
/// 64-KiB pages; tables contain nullable function references. Host callbacks are wrapped by
/// [`HostFunction`]; callable exports retain their signature through [`ExportFunction`].
///
/// Instances retain their imports. Exported functions and table entries remain
/// valid after the original module and instance handles are dropped.
pub trait WasmBackend: WasmMemoryAccess + Sized {
    type State: 'static;
    type Context<'a>: WasmContext<State = Self::State, Memory = Self::Memory>
    where
        Self: 'a;
    /// Owns exclusive access across calls and lends it to synchronous callbacks.
    type Execution<'a>: WasmContext<State = Self::State, Memory = Self::Memory>
    where
        Self: 'a;
    /// Cloning a module shares compiled code without creating a guest instance.
    type Module: Clone + 'static;
    type Compiler: WasmCompiler<Module = Self::Module>;
    type Instance;
    type Table: 'static;
    /// An engine function handle produced by a typed binding.
    type ImportFunction: Clone;
    /// Retains a shared allocation independently of this backend.
    #[cfg(not(target_arch = "wasm32"))]
    type SharedMemory: WasmSharedMemory + Send + Sync;
    #[cfg(target_arch = "wasm32")]
    type SharedMemory: WasmSharedMemory;

    /// Host state is borrowed only between Wasm calls. A browser backend may
    /// return a borrow guard; drop it before calling into Wasm.
    fn state(&self) -> impl Deref<Target = Self::State>;
    fn state_mut(&mut self) -> impl DerefMut<Target = Self::State>;

    /// Borrows the backend for execution and host access between calls. Use
    /// [`Function::call`] to enter Wasm through this owner. It may be held
    /// across an await while the guest is stopped.
    fn execution(&mut self) -> Self::Execution<'_>;

    /// Returns an independent compiler whose modules this backend can instantiate.
    fn compiler(&self) -> Self::Compiler;

    /// Optional storage for modules that callers choose to retain.
    /// Share it only between backends that can instantiate each other's modules.
    /// Ordinary [`WasmCompiler::compile`] calls do not populate this cache.
    fn module_cache(&self) -> Option<&ModuleCache<Self::Module>> {
        None
    }

    /// Allocates zeroed memory with the declaration's limits and sharing mode.
    /// Shared memory requires a maximum. Import names are used only when linking.
    fn create_memory(&mut self, declaration: MemoryDeclaration) -> Result<Self::Memory>;

    /// Obtains an independently owned handle for shared memory.
    /// Returns `None` for private memory.
    fn shared_memory(&self, memory: &Self::Memory) -> Option<Self::SharedMemory>;

    /// Attaches an existing shared allocation to this backend. The returned handle
    /// belongs to this backend; the allocation remains shared with its other users.
    fn import_shared_memory(&mut self, memory: &Self::SharedMemory) -> Self::Memory;

    /// Grows memory when needed to contain `byte_len` bytes, preserving its contents.
    fn ensure_memory_size(&mut self, memory: &Self::Memory, byte_len: u64) -> Result<()>;

    /// Allocates a function table whose entries initially contain null.
    fn create_table(&mut self, minimum: u32) -> Result<Self::Table>;

    /// Grows the table to at least `minimum` slots, preserving existing functions.
    /// New slots contain null. A smaller minimum leaves the table unchanged.
    fn ensure_table_size(&mut self, table: &Self::Table, minimum: u32) -> Result<()>;

    /// Binds a memory, table or previously resolved function with the same signature.
    ///
    /// Rebinding cannot change the function's logical widths:
    /// ```compile_fail
    /// use relocade_compiler::abi::Function as Declaration;
    /// use relocade_wasm_runtime::{Backend, Function, WasmBackend};
    /// fn wrong(backend: &Backend, function: &Function<Backend, fn(u16) -> u32>) {
    ///     const WIDE: Declaration<fn(u32) -> u32> = Declaration::new("example", "wide");
    ///     backend.bind(WIDE, function);
    /// }
    /// ```
    fn bind<'a, D: ImportBinding<'a, Self>>(
        &self,
        declaration: D,
        value: D::Value,
    ) -> Import<'a, Self>
    where
        Self: 'a,
    {
        declaration.bind(self, value)
    }

    /// Wraps a callback in the engine and binds it to the shared declaration.
    /// Callbacks receive a scoped context followed by the declared Rust values.
    /// The shared ABI codec converts their Wasm representations, including
    /// zero-extension of signed narrow values. Logical marker declarations retain
    /// their canonical Rust carrier values.
    /// Native callbacks implement Fn + Send + Sync; browser callbacks stay on
    /// their JavaScript thread. Mutable environment state is accessed through
    /// the context. Memory handles can be captured normally.
    /// Linked modules retain callbacks for as long as they can call them.
    /// The import owns its handle without borrowing the backend.
    ///
    /// ```
    /// use relocade_compiler::abi::Function;
    /// use relocade_wasm_runtime::{HostFunction, WasmBackend};
    /// const READ: Function<fn(u16, u32) -> u32> = Function::new("example", "read");
    /// fn bind<B: HostFunction<fn(u16, u32) -> u32>>(backend: &mut B) {
    ///     let _ = backend.bind_host(READ, Box::new(|_, port, width| u32::from(port) + width));
    /// }
    /// ```
    ///
    /// A callback with a different return type is rejected by Rust:
    /// ```compile_fail
    /// use relocade_compiler::{abi::Function, I16, I32};
    /// use relocade_wasm_runtime::{HostFunction, WasmBackend};
    /// const READ: Function<fn(I16, I32) -> I32> = Function::new("example", "read");
    /// fn bind<B: HostFunction<fn(I16, I32) -> I32>>(backend: &mut B) {
    ///     let _ = backend.bind_host(READ, Box::new(|_, _, _| 0_i64));
    /// }
    /// ```
    /// Parameter types must match too:
    /// ```compile_fail
    /// use relocade_compiler::{abi::Function, I32, I64};
    /// use relocade_wasm_runtime::{HostFunction, WasmBackend};
    /// const WRITE: Function<fn(I32, I32, I64)> = Function::new("example", "write");
    /// fn bind<B: HostFunction<fn(I32, I32, I64)>>(backend: &mut B) {
    ///     let _ = backend.bind_host(WRITE, Box::new(|_, _, _, value: i32| {}));
    /// }
    /// ```
    fn bind_host<'a, S: HostSignature<Self>>(
        &mut self,
        declaration: FunctionDeclaration<'a, S>,
        callback: S::Callback,
    ) -> Import<'a, Self>
    where
        Self: HostFunction<S>,
    {
        Import::function(declaration, self.host_function(callback))
    }

    /// Obtains the import handle without discarding the cached callable export.
    fn import_function<S: CallSignature>(
        &self,
        function: &Function<Self, S>,
    ) -> Self::ImportFunction
    where
        Self: ExportFunction<S>,
    {
        self.function_import(&function.handle)
    }

    fn instantiate(
        &mut self,
        module: &Self::Module,
        imports: &[Import<'_, Self>],
    ) -> impl Future<Output = Result<Self::Instance>>;

    /// Resolves an export using the same declaration as the generated function.
    /// The declaration must describe that export. Logical widths are erased in
    /// Wasm; both backends check the physical signature when resolving it.
    fn function<S: CallSignature>(
        &mut self,
        instance: &Self::Instance,
        declaration: FunctionDeclaration<S>,
    ) -> Result<Function<Self, S>>
    where
        Self: ExportFunction<S>,
    {
        self.resolve_function(instance, declaration)
            .map(Function::new)
    }

    /// Installs a function in an existing table slot.
    fn set_table_function<S: CallSignature>(
        &mut self,
        table: &Self::Table,
        index: u32,
        function: &Function<Self, S>,
    ) -> Result<()>
    where
        Self: ExportFunction<S>,
    {
        let function = self.import_function(function);
        self.set_table_import(table, index, function)
    }

    /// Installs an engine handle supplied by a typed function binding.
    #[doc(hidden)]
    fn set_table_import(
        &mut self,
        table: &Self::Table,
        index: u32,
        function: Self::ImportFunction,
    ) -> Result<()>;

    /// Calls a cached export with ordinary Rust arguments and results.
    /// Arguments form a tuple, including `()` for no arguments and `(value,)`
    /// for one argument. No export lookup is performed during the call.
    /// Wasm traps and invalid ABI values are fatal programming errors.
    ///
    /// ```
    /// use relocade_compiler::{abi::Function, Program};
    /// use relocade_wasm_runtime::{Backend, WasmBackend, WasmCompiler};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// const ADD: Function<fn(u32, u32) -> u32> = Function::new("example", "add");
    /// let mut program = Program::new();
    /// ADD.define(&mut program, move |body, (a, b)| body.return_(a.add(b)))?;
    /// let mut backend = Backend::new()?;
    /// let module = backend.compiler().compile(&program.compile()?).await?;
    /// let instance = backend.instantiate(&module, &[]).await?;
    /// let add = backend.function(&instance, ADD)?;
    /// assert_eq!(backend.call(&add, (u32::MAX, 2)), 1);
    /// # Ok(()) }
    /// ```
    ///
    /// Argument values must match the cached signature:
    /// ```compile_fail
    /// use relocade_wasm_runtime::{Backend, Function, WasmBackend};
    /// fn wrong(backend: &mut Backend, function: &Function<Backend, fn(u16) -> bool>) {
    ///     backend.call(function, (1_u32,));
    /// }
    /// ```
    fn call<S: CallSignature>(
        &mut self,
        function: &Function<Self, S>,
        arguments: Value<S::Parameters>,
    ) -> Value<S::Return>
    where
        Self: ExportFunction<S>,
    {
        function.call(&mut self.execution(), arguments)
    }
}
