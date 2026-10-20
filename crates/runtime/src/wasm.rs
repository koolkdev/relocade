//! Wasm engine operations used by the runtime. Compiled code may be reused across
//! compatible backends. Instances bind to one backend's memories, tables and functions.

use crate::Result;
use relocade_compiler::{
    abi::{Function, Memory},
    I64,
};
use std::future::Future;
use std::ops::{Deref, DerefMut};

mod host;
mod imports;
mod module_cache;
pub use host::{HostFunction, HostSignature, HostValue};
pub use imports::{Import, ImportBinding, ImportValue};
pub use module_cache::ModuleCache;

/// Access to linear memory through a Wasm backend or its scoped context.
/// Handles belong to that engine. Transfers require an allocated, in-bounds
/// range; violating this storage invariant is a programming error.
pub trait WasmMemoryAccess {
    /// A handle to engine-owned memory that host functions can capture. Access to
    /// the contents still requires the owning backend or its scoped context.
    type Memory: Clone + Send + Sync + 'static;

    fn read_memory(&self, memory: &Self::Memory, offset: u32, bytes: &mut [u8]);
    fn write_memory(&mut self, memory: &Self::Memory, offset: u32, bytes: &[u8]);
}

impl<A: WasmMemoryAccess + ?Sized> WasmMemoryAccess for &mut A {
    type Memory = A::Memory;

    fn read_memory(&self, memory: &Self::Memory, offset: u32, bytes: &mut [u8]) {
        (**self).read_memory(memory, offset, bytes);
    }

    fn write_memory(&mut self, memory: &Self::Memory, offset: u32, bytes: &[u8]) {
        (**self).write_memory(memory, offset, bytes);
    }
}

/// An independently owned reader for shared linear memory. Reads may overlap
/// guest or host writes and do not provide a consistent snapshot of a range.
/// The range must be allocated and in bounds, as with [`WasmMemoryAccess`].
///
/// Native handles can cross threads. Browser handles stay on their JavaScript
/// thread; the underlying shared WebAssembly.Memory can be sent to a worker.
pub trait WasmSharedMemory: Clone + 'static {
    fn read(&self, offset: u32, bytes: &mut [u8]);
}

/// Scoped access to host state and live Wasm memory, during a callback or while
/// execution is stopped. A stopped view may be held across an await; drop it
/// before entering Wasm again. Callback views cannot outlive their callback.
pub trait WasmContext: WasmMemoryAccess {
    type State;

    fn state(&self) -> &Self::State;
    fn state_mut(&mut self) -> &mut Self::State;
}

/// One Wasm resource owner. Preparation futures, environment state and handles
/// other than memory need not be `Send`. Memory is Wasm32 with
/// 64-KiB pages; tables contain nullable function references. Host callbacks are wrapped by
/// [`HostFunction`]; the runtime's callable exports use `() -> i64`.
///
/// Instances retain their imports. Exported functions and table entries remain
/// valid after the original module and instance handles are dropped.
pub trait WasmBackend: WasmMemoryAccess + Sized {
    type State: 'static;
    type Context<'a>: WasmContext<State = Self::State, Memory = Self::Memory>
    where
        Self: 'a;
    /// Cloning a module shares compiled code without creating a guest instance.
    type Module: Clone + 'static;
    type Instance;
    type Table: 'static;
    /// An exported function with signature `() -> i64`.
    type Function;
    /// An engine function handle, after its binding's signature has been checked.
    type ImportFunction: Clone;
    type SharedMemory: WasmSharedMemory;

    /// Host state is borrowed only between Wasm calls. A browser backend may
    /// return a borrow guard; drop it before calling into Wasm.
    fn state(&self) -> impl Deref<Target = Self::State>;
    fn state_mut(&mut self) -> impl DerefMut<Target = Self::State>;

    /// Borrows the stopped backend's state and memory. The view may be held
    /// across an await, but must be dropped before calling into Wasm again.
    fn context(&mut self) -> Self::Context<'_>;

    /// Compiles Wasm bytes into code that can be instantiated with private imports.
    fn compile(&self, bytes: &[u8]) -> impl Future<Output = Result<Self::Module>>;

    /// Optional background compiler sharing this backend's engine compatibility.
    /// It must not retain or borrow the live store, guest memory or host state.
    fn create_compiler(
        &self,
    ) -> Result<Option<Box<dyn crate::compilation::BackgroundCompiler<Self::Module>>>> {
        Ok(None)
    }

    /// Yields execution to the embedding's scheduler. Browser implementations
    /// must yield to an event-loop task, so worker messages can be delivered.
    fn yield_execution(&self) -> impl Future<Output = Result<()>> {
        async {
            let mut yielded = false;
            std::future::poll_fn(|cx| {
                if yielded {
                    std::task::Poll::Ready(Ok(()))
                } else {
                    yielded = true;
                    cx.waker().wake_by_ref();
                    std::task::Poll::Pending
                }
            })
            .await
        }
    }

    /// Optional storage for modules that callers choose to retain.
    /// Share it only between backends that can instantiate each other's modules.
    /// Ordinary `compile` calls do not populate this cache.
    fn module_cache(&self) -> Option<&ModuleCache<Self::Module>> {
        None
    }

    /// Allocates zeroed memory with the declaration's limits and sharing mode.
    /// Shared memory requires a maximum. Import names are used only when linking.
    fn create_memory(&mut self, declaration: Memory) -> Result<Self::Memory>;

    /// Obtains a reader that does not borrow the backend or its context.
    /// Returns `None` for private memory.
    fn shared_memory(&self, memory: &Self::Memory) -> Option<Self::SharedMemory>;

    /// Grows memory when needed to contain `byte_len` bytes, preserving its contents.
    fn ensure_memory_size(&mut self, memory: &Self::Memory, byte_len: u64) -> Result<()>;

    /// Allocates a function table whose entries initially contain null.
    fn create_table(&mut self, minimum: u32) -> Result<Self::Table>;

    /// Grows the table to at least `minimum` slots, preserving existing functions.
    /// New slots contain null. A smaller minimum leaves the table unchanged.
    fn ensure_table_size(&mut self, table: &Self::Table, minimum: u32) -> Result<()>;

    /// Binds a memory, table or previously resolved `() -> i64` function.
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
    /// Callbacks receive a scoped context followed by the Wasm carrier arguments;
    /// narrow integers use zero-extended i32, as in the compiler's import ABI.
    /// Callbacks must also return narrow results zero-extended to their logical width.
    /// Callbacks implement Fn + Send + Sync; mutable or non-Send environment state
    /// is accessed through the context. Memory handles can be captured normally.
    /// The backend retains callbacks while linked modules can call them.
    /// The import owns its handle without borrowing the backend.
    ///
    /// ```
    /// use relocade_compiler::{abi::Function, I16, I32};
    /// use relocade_runtime::wasm::{HostFunction, WasmBackend};
    /// const READ: Function<fn(I16, I32) -> I32> = Function::new("example", "read");
    /// fn bind<B: HostFunction<fn(I16, I32) -> I32>>(backend: &mut B) {
    ///     let _ = backend.bind_host(READ, Box::new(|_, port, width| port + width));
    /// }
    /// ```
    ///
    /// A callback with a different return type is rejected by Rust:
    /// ```compile_fail
    /// use relocade_compiler::{abi::Function, I16, I32};
    /// use relocade_runtime::wasm::{HostFunction, WasmBackend};
    /// const READ: Function<fn(I16, I32) -> I32> = Function::new("example", "read");
    /// fn bind<B: HostFunction<fn(I16, I32) -> I32>>(backend: &mut B) {
    ///     let _ = backend.bind_host(READ, Box::new(|_, _, _| 0_i64));
    /// }
    /// ```
    /// Parameter types must match too:
    /// ```compile_fail
    /// use relocade_compiler::{abi::Function, I32, I64};
    /// use relocade_runtime::wasm::{HostFunction, WasmBackend};
    /// const WRITE: Function<fn(I32, I32, I64)> = Function::new("example", "write");
    /// fn bind<B: HostFunction<fn(I32, I32, I64)>>(backend: &mut B) {
    ///     let _ = backend.bind_host(WRITE, Box::new(|_, _, _, value: i32| {}));
    /// }
    /// ```
    fn bind_host<'a, S: HostSignature<Self>>(
        &mut self,
        declaration: Function<'a, S>,
        callback: S::Callback,
    ) -> Import<'a, Self>
    where
        Self: HostFunction<S>,
    {
        Import::function(declaration, self.host_function(callback))
    }

    /// Obtains the import handle without discarding the cached callable export.
    fn import_function(&self, function: &Self::Function) -> Self::ImportFunction;

    fn instantiate(
        &mut self,
        module: &Self::Module,
        imports: &[Import<'_, Self>],
    ) -> impl Future<Output = Result<Self::Instance>>;

    fn function(
        &mut self,
        instance: &Self::Instance,
        declaration: Function<fn() -> I64>,
    ) -> Result<Self::Function>;

    /// Installs a function in an existing table slot.
    fn set_table_function(
        &mut self,
        table: &Self::Table,
        index: u32,
        function: &Self::Function,
    ) -> Result<()>;

    /// Calls a previously resolved `() -> i64` export without another lookup.
    fn call(&mut self, function: &Self::Function) -> Result<i64>;
}
