use crate::{
    Error, Import, ImportValue, MemoryAccess, ModuleCache, Result, WasmBackend, WasmMemoryAccess,
};
use relocade_compiler::abi;
use std::ops::{Deref, DerefMut};
use std::{marker::PhantomData, rc::Rc};
use wasmtime::{AsContextMut, Func, Instance, Linker, Store, Table};

/// Compiled code shared by compatible backends and transferable to another worker.
#[derive(Clone, Debug)]
pub struct Module(wasmtime::Module);

impl PartialEq for Module {
    fn eq(&self, other: &Self) -> bool {
        wasmtime::Module::same(&self.0, &other.0)
    }
}

impl Eq for Module {}

mod calls;
mod compilation;
mod context;
mod host;
mod memory;
mod tables;
pub use compilation::Compiler;
pub use context::Context;
pub(crate) use memory::MemoryRegion;
pub use memory::{Memory, SharedMemory};

/// One store for guest instances and their imports. Runtime owns execution policy.
pub struct Backend<T: 'static = ()> {
    store: Store<StoreData<T>>,
    compilation: &'static compilation::Compilation,
}

// Host functions can own thread-local captures even when the user's state is Send.
// Keep every store view on its creation thread, including callback contexts.
struct StoreData<T> {
    state: T,
    local: PhantomData<Rc<()>>,
}

impl Backend {
    pub fn new() -> Result<Self> {
        Self::with_state(())
    }
}

impl<T: 'static> Backend<T> {
    pub fn with_state(state: T) -> Result<Self> {
        let compilation = compilation::shared()?;
        Ok(Self {
            store: Store::new(
                compilation.engine(),
                StoreData {
                    state,
                    local: PhantomData,
                },
            ),
            compilation,
        })
    }
}

impl<T: 'static> WasmBackend for Backend<T> {
    type State = T;
    type Context<'a> = Context<'a, T>;
    type Module = Module;
    type Compiler = Compiler;
    type Instance = Instance;
    type Table = Table;
    type ImportFunction = Func;
    type SharedMemory = SharedMemory;

    fn state(&self) -> impl Deref<Target = T> {
        &self.store.data().state
    }

    fn state_mut(&mut self) -> impl DerefMut<Target = T> {
        &mut self.store.data_mut().state
    }

    fn context(&mut self) -> Context<'_, T> {
        Context(self.store.as_context_mut())
    }

    fn compiler(&self) -> Compiler {
        self.compilation.compiler()
    }

    fn module_cache(&self) -> Option<&ModuleCache<Module>> {
        Some(&self.compilation.modules)
    }

    fn create_memory(&mut self, declaration: abi::Memory) -> Result<Memory> {
        Memory::new(&mut self.store, declaration)
    }

    fn shared_memory(&self, memory: &Memory) -> Option<SharedMemory> {
        match memory {
            Memory::Private(_) => None,
            Memory::Shared(memory) => Some(memory.clone()),
        }
    }

    fn import_shared_memory(&mut self, memory: &SharedMemory) -> Memory {
        Memory::Shared(memory.clone())
    }

    fn allocate_table(
        &mut self,
        minimum: u32,
        maximum: Option<u32>,
        initial: Option<&Func>,
    ) -> Result<Table> {
        tables::create(&mut self.store, minimum, maximum, initial)
    }

    fn ensure_memory_size(&mut self, memory: &Memory, byte_len: u64) -> Result<()> {
        memory.ensure_size(&mut self.store, byte_len)
    }

    fn instantiate(&mut self, module: &Module, imports: &[Import<'_, Self>]) -> Result<Instance> {
        let mut linker = Linker::new(self.store.engine());
        for import in imports {
            let (module, name) = (import.module(), import.name());
            match import.value() {
                ImportValue::Memory(memory) => {
                    linker.define(&self.store, module, name, memory.as_extern())
                }
                ImportValue::Table(table) => linker.define(&self.store, module, name, **table),
                ImportValue::Function(function) => {
                    linker.define(&self.store, module, name, *function)
                }
            }
            .map_err(|error| Error::new("bind Wasm import", error))?;
        }
        linker
            .instantiate(&mut self.store, &module.0)
            .map_err(|error| Error::new("instantiate Wasm", error))
    }
}

impl<T: 'static> WasmMemoryAccess for Backend<T> {
    type Memory = Memory;

    fn memory<'a>(&'a mut self, memory: &'a Memory) -> MemoryAccess<'a> {
        memory.access(self.store.as_context_mut())
    }
}
