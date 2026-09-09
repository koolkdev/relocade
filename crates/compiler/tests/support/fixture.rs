use wasm86_compiler::{
    BuildError, Func, FunctionBuilder, FunctionImport, IntType, Mem, MemoryImport, Program,
    Signature, Type, Val,
};

use crate::wasm::{Callback, MemoryBytes, TestModule, Value};

pub fn signature(parameters: &[Type], result: Option<Type>) -> Signature {
    Signature {
        parameters: parameters.into(),
        result,
    }
}

/// Keep authored imports and their host configuration in the same fixture.
#[derive(Default)]
pub struct Fixture {
    pub program: Program,
    memories: Vec<MemoryBytes>,
    callbacks: Vec<Callback>,
}

impl Fixture {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn memory(&mut self, name: &str, bytes: &[u8]) -> Mem {
        self.memories.push(MemoryBytes::new(name, bytes));
        self.program.import_memory(MemoryImport {
            module: "test".into(),
            name: name.into(),
            minimum: 1,
            maximum: None,
        })
    }

    pub fn callback(&mut self, name: &str, signature: Signature, result: Option<Value>) -> Func {
        self.callbacks.push(match result {
            Some(value) => Callback::new(name, value),
            None => Callback::void(name),
        });
        self.program.import_function(FunctionImport {
            module: "test".into(),
            name: name.into(),
            signature,
        })
    }

    /// Build the common single-function fixture with the public function API.
    pub fn function(
        mut self,
        parameters: &[Type],
        result: Option<Type>,
        build: impl FnOnce(FunctionBuilder<'_>) -> Result<(), BuildError>,
    ) -> TestModule {
        let run = self
            .program
            .function(signature(parameters, result), build)
            .expect("build fixture function");
        self.finish(run)
    }

    pub fn expression<T: IntType>(
        self,
        parameters: &[Type],
        build: impl FnOnce(&FunctionBuilder<'_>) -> Val<T>,
    ) -> TestModule {
        self.function(parameters, Some(T::TYPE), |body| {
            let value = build(&body);
            body.return_(value)
        })
    }

    /// Complete a fixture whose bodies need explicit declarations or multiple functions.
    pub fn finish(mut self, run: Func) -> TestModule {
        self.program
            .export("run", run)
            .expect("export fixture entry");
        self.compile()
    }

    pub fn compile(self) -> TestModule {
        let bytes = self.program.compile().expect("compile fixture");
        TestModule::with_imports(&bytes, self.memories, self.callbacks)
    }
}
