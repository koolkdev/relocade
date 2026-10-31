use crate::{
    Error, Import, ImportValue, MemoryAccess, ModuleCache, Result, WasmBackend, WasmMemoryAccess,
};
use relocade_compiler::abi;
use std::ops::{Deref, DerefMut};
use wasmtime::{AsContextMut, Func, Instance, Linker, Ref, RefType, Store, Table, TableType};

pub use wasmtime::Module;

mod calls;
mod compilation;
mod context;
mod host;
mod memory;
pub use compilation::Compiler;
pub use context::Context;
pub(crate) use memory::{FixedMemoryRegion, MemoryRegion};
pub use memory::{Memory, SharedMemory};

/// One store for guest instances and their imports. Runtime owns execution policy.
pub struct Backend<T: 'static = ()> {
    store: Store<T>,
    compilation: &'static compilation::Compilation,
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
            store: Store::new(compilation.engine(), state),
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
        self.store.data()
    }

    fn state_mut(&mut self) -> impl DerefMut<Target = T> {
        self.store.data_mut()
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

    fn create_table(&mut self, minimum: u32) -> Result<Table> {
        Table::new(
            &mut self.store,
            TableType::new(RefType::FUNCREF, minimum, None),
            Ref::Func(None),
        )
        .map_err(|error| Error::new("create Wasm table", error))
    }

    fn ensure_table_size(&mut self, table: &Table, minimum: u32) -> Result<()> {
        let minimum = u64::from(minimum);
        let current = table.size(&self.store);
        if minimum > current {
            table
                .grow(&mut self.store, minimum - current, Ref::Func(None))
                .map_err(|error| Error::new("grow Wasm table", error))?;
        }
        Ok(())
    }

    fn ensure_memory_size(&mut self, memory: &Memory, byte_len: u64) -> Result<()> {
        memory.ensure_size(&mut self.store, byte_len)
    }

    async fn instantiate(
        &mut self,
        module: &Module,
        imports: &[Import<'_, Self>],
    ) -> Result<Instance> {
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
            .instantiate(&mut self.store, module)
            .map_err(|error| Error::new("instantiate Wasm", error))
    }

    fn set_table_import(&mut self, table: &Table, index: u32, function: Func) -> Result<()> {
        table
            .set(&mut self.store, u64::from(index), Ref::Func(Some(function)))
            .map_err(|error| Error::new("set Wasm table function", error))
    }
}

impl<T: 'static> WasmMemoryAccess for Backend<T> {
    type Memory = Memory;

    fn memory<'a>(&'a mut self, memory: &'a Memory) -> MemoryAccess<'a> {
        memory.access(self.store.as_context_mut())
    }
}
