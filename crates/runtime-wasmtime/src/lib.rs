//! Wasmtime implementation of the runtime's Wasm engine interface.
//!
//! ```
//! use wasm86_machine::MachineConfig;
//! use wasm86_runtime::Runtime;
//! use wasm86_runtime_wasmtime::WasmtimeBackend;
//! use wasm86_x86::{Compiler, CpuState, ExecutionExit, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap, Segments};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let target = Compiler::new(ExecutionProfile::Real16).target();
//! let cpu = CpuState { eip: 0x100, segments: Segments::real_mode(), ..CpuState::default() };
//! let map = PhysicalMemoryMap::new([(0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 })])?;
//! let config = MachineConfig::new(target, cpu, 0x10000, map)?;
//! let mut runtime = Runtime::new(WasmtimeBackend::new()?, config).await?;
//! runtime.machine().memory().write(0x100, &[0xb8, 42, 0, 0xeb, 0])?;
//! assert_eq!(runtime.run(1)?, ExecutionExit::BudgetExhausted);
//! assert_eq!(runtime.machine().cpu().eax(), 42);
//! # Ok(())
//! # }
//! ```
#![forbid(unsafe_code)]
#![cfg(not(target_arch = "wasm32"))]

use std::ops::{Deref, DerefMut};
use wasm86_compiler::{abi::Function, I64};
use wasm86_runtime::{
    wasm::{Import, ImportValue, WasmBackend, WasmMemoryAccess},
    Error, Result,
};
use wasmtime::{
    Config, Engine, Func, Instance, Linker, Memory, MemoryType, Module, Ref, RefType, Store, Table,
    TableType, TypedFunc,
};

mod host;
pub use host::Caller;

/// One store for modules and their shared imports. Runtime owns execution policy.
pub struct WasmtimeBackend<T = ()> {
    store: Store<T>,
}

impl WasmtimeBackend {
    pub fn new() -> Result<Self> {
        Self::with_state(())
    }
}

impl<T: 'static> WasmtimeBackend<T> {
    pub fn with_state(state: T) -> Result<Self> {
        let mut config = Config::new();
        config
            .wasm_multi_memory(true)
            .wasm_tail_call(true)
            .wasm_threads(true);
        let engine =
            Engine::new(&config).map_err(|error| Error::wasm("create Wasmtime engine", error))?;
        Ok(Self {
            store: Store::new(&engine, state),
        })
    }
}

impl<T: 'static> WasmBackend for WasmtimeBackend<T> {
    type State = T;
    type Caller<'a> = Caller<'a, T>;
    type Module = Module;
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

    /// Native compilation completes synchronously on the calling thread.
    async fn compile(&self, bytes: &[u8]) -> Result<Module> {
        Module::new(self.store.engine(), bytes).map_err(|error| Error::wasm("compile Wasm", error))
    }

    fn create_memory(&mut self, minimum: u32, maximum: Option<u32>) -> Result<Memory> {
        Memory::new(&mut self.store, MemoryType::new(minimum, maximum))
            .map_err(|error| Error::wasm("create Wasm memory", error))
    }

    fn create_table(&mut self, minimum: u32) -> Result<Table> {
        Table::new(
            &mut self.store,
            TableType::new(RefType::FUNCREF, minimum, None),
            Ref::Func(None),
        )
        .map_err(|error| Error::wasm("create Wasm table", error))
    }

    fn ensure_table_size(&mut self, table: &Table, minimum: u32) -> Result<()> {
        let minimum = u64::from(minimum);
        let current = table.size(&self.store);
        if minimum > current {
            table
                .grow(&mut self.store, minimum - current, Ref::Func(None))
                .map_err(|error| Error::wasm("grow Wasm table", error))?;
        }
        Ok(())
    }

    fn ensure_memory_size(&mut self, memory: &Memory, byte_len: u64) -> Result<()> {
        let pages = byte_len.div_ceil(65536);
        let current = memory.size(&self.store);
        if pages > current {
            memory
                .grow(&mut self.store, pages - current)
                .map_err(|error| Error::wasm("grow Wasm memory", error))?;
        }
        Ok(())
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
                ImportValue::Memory(memory) => linker.define(&self.store, module, name, **memory),
                ImportValue::Table(table) => linker.define(&self.store, module, name, **table),
                ImportValue::Function(function) => {
                    linker.define(&self.store, module, name, *function)
                }
            }
            .map_err(|error| Error::wasm("bind Wasm import", error))?;
        }
        linker
            .instantiate(&mut self.store, module)
            .map_err(|error| Error::wasm("instantiate Wasm", error))
    }

    fn function(
        &mut self,
        instance: &Instance,
        declaration: Function<fn() -> I64>,
    ) -> Result<Self::Function> {
        instance
            .get_typed_func(&mut self.store, declaration.name)
            .map_err(|error| Error::wasm("resolve Wasm function", error))
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
            .map_err(|error| Error::wasm("set Wasm table function", error))
    }

    fn call(&mut self, function: &Self::Function) -> Result<i64> {
        function
            .call(&mut self.store, ())
            .map_err(|error| Error::wasm("call Wasm function", error))
    }
}

impl<T> WasmMemoryAccess for WasmtimeBackend<T> {
    type Memory = Memory;

    fn read_memory(&self, memory: &Memory, offset: u32, bytes: &mut [u8]) {
        memory
            .read(&self.store, offset as usize, bytes)
            .expect("in-bounds Wasm memory range");
    }

    fn write_memory(&mut self, memory: &Memory, offset: u32, bytes: &[u8]) {
        memory
            .write(&mut self.store, offset as usize, bytes)
            .expect("in-bounds Wasm memory range");
    }
}
