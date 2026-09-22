//! Safe test-host access to shared memories through a small Wasm byte accessor.

use wasm_encoder::{
    CodeSection, EntityType, ExportKind, ExportSection, Function, FunctionSection, ImportSection,
    Instruction, MemArg, Module, TypeSection, ValType,
};
use wasmtime::{Linker, MemoryType, SharedMemory, Store};

/// Shared bytes whose setup and inspection use Wasm atomic byte accesses. Tests
/// coordinate whole snapshots themselves; a multi-byte copy is not atomic.
#[derive(Clone)]
pub struct SharedBytes {
    memory: SharedMemory,
    accessor: wasmtime::Module,
}

impl SharedBytes {
    pub fn new(minimum: u32, maximum: u32) -> Self {
        let engine = crate::engine();
        let memory = SharedMemory::new(engine, MemoryType::shared(minimum, maximum)).unwrap();
        let mut types = TypeSection::new();
        types.ty().function([ValType::I32], [ValType::I32]);
        types.ty().function([ValType::I32, ValType::I32], []);
        let mut imports = ImportSection::new();
        imports.import(
            "test",
            "memory",
            EntityType::Memory(wasm_encoder::MemoryType {
                minimum: u64::from(minimum),
                maximum: Some(u64::from(maximum)),
                memory64: false,
                shared: true,
                page_size_log2: None,
            }),
        );
        let mut functions = FunctionSection::new();
        functions.function(0).function(1);
        let mut exports = ExportSection::new();
        exports
            .export("read", ExportKind::Func, 0)
            .export("write", ExportKind::Func, 1);
        let argument = MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        };
        let mut read = Function::new([]);
        read.instruction(&Instruction::LocalGet(0))
            .instruction(&Instruction::I32AtomicLoad8U(argument))
            .instruction(&Instruction::End);
        let mut write = Function::new([]);
        write
            .instruction(&Instruction::LocalGet(0))
            .instruction(&Instruction::LocalGet(1))
            .instruction(&Instruction::I32AtomicStore8(argument))
            .instruction(&Instruction::End);
        let mut code = CodeSection::new();
        code.function(&read).function(&write);
        let mut module = Module::new();
        module
            .section(&types)
            .section(&imports)
            .section(&functions)
            .section(&exports)
            .section(&code);
        let accessor = wasmtime::Module::new(engine, module.finish()).unwrap();
        Self { memory, accessor }
    }

    pub fn memory(&self) -> &SharedMemory {
        &self.memory
    }

    pub fn read(&self, offset: u32, length: usize) -> Vec<u8> {
        let (mut store, instance) = self.accessor();
        let read = instance
            .get_typed_func::<u32, u32>(&mut store, "read")
            .unwrap();
        (0..length)
            .map(|index| {
                read.call(&mut store, offset + u32::try_from(index).unwrap())
                    .unwrap() as u8
            })
            .collect()
    }

    pub fn write(&self, offset: u32, bytes: &[u8]) {
        let (mut store, instance) = self.accessor();
        let write = instance
            .get_typed_func::<(u32, u32), ()>(&mut store, "write")
            .unwrap();
        for (index, &byte) in bytes.iter().enumerate() {
            write
                .call(
                    &mut store,
                    (offset + u32::try_from(index).unwrap(), u32::from(byte)),
                )
                .unwrap();
        }
    }

    fn accessor(&self) -> (Store<()>, wasmtime::Instance) {
        let mut store = Store::new(crate::engine(), ());
        let mut linker = Linker::new(crate::engine());
        linker
            .define(&store, "test", "memory", self.memory.clone())
            .unwrap();
        let instance = linker.instantiate(&mut store, &self.accessor).unwrap();
        (store, instance)
    }
}
