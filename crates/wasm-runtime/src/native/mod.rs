use crate::{
    Error, Import, ImportValue, MemoryBytes, ModuleCache, Result, WasmBackend, WasmMemoryAccess,
};
use relocade_compiler::{
    abi::{self, Function},
    I64,
};
use std::ops::{Deref, DerefMut};
use wasmtime::{
    AsContextMut, Func, Instance, Linker, Ref, RefType, Store, Table, TableType, TypedFunc,
};

pub use wasmtime::Module;

mod compilation;
mod host;
mod memory;
pub use compilation::Compiler;
pub use host::Context;
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
    type Function = TypedFunc<(), i64>;
    type ImportFunction = Func;

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

    fn function(
        &mut self,
        instance: &Instance,
        declaration: Function<fn() -> I64>,
    ) -> Result<Self::Function> {
        instance
            .get_typed_func(&mut self.store, declaration.name)
            .map_err(|error| Error::new("resolve Wasm function", error))
    }

    fn import_function(&self, function: &Self::Function) -> Func {
        *function.func()
    }

    fn set_table_function(
        &mut self,
        table: &Table,
        index: u32,
        function: &Self::Function,
    ) -> Result<()> {
        table
            .set(
                &mut self.store,
                u64::from(index),
                Ref::Func(Some(*function.func())),
            )
            .map_err(|error| Error::new("set Wasm table function", error))
    }

    fn call(&mut self, function: &Self::Function) -> Result<i64> {
        function
            .call(&mut self.store, ())
            .map_err(|error| Error::new("call Wasm function", error))
    }
}

impl<T: 'static> WasmMemoryAccess for Backend<T> {
    type Memory = Memory;
    type SharedMemory = SharedMemory;

    fn memory_bytes<'a>(&'a mut self, memory: &'a Memory) -> MemoryBytes<'a> {
        memory.bytes(&mut self.store)
    }

    fn shared_memory(&self, memory: &Memory) -> Option<SharedMemory> {
        match memory {
            Memory::Private(_) => None,
            Memory::Shared(memory) => Some(memory.clone()),
        }
    }

    fn read_memory(&self, memory: &Memory, offset: u32, bytes: &mut [u8]) {
        memory.read(&self.store, offset, bytes);
    }

    fn write_memory(&mut self, memory: &Memory, offset: u32, bytes: &[u8]) {
        memory.write(&mut self.store, offset, bytes);
    }
}
