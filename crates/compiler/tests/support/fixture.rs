use relocade_compiler::{
    BlockBuilder, BuildError, Func, FunctionImport, Mem, MemoryImport, Program, Signature, Type,
    Val, ValueType,
};

use crate::wasm::{Callback, MemoryBytes, TestModule, Value};

pub fn signature(parameters: &[Type], results: &[Type]) -> Signature {
    Signature {
        parameters: parameters.into(),
        results: results.into(),
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
        self.memory_import(name, bytes, false)
    }

    pub fn shared_memory(&mut self, name: &str, bytes: &[u8]) -> Mem {
        self.memory_import(name, bytes, true)
    }

    fn memory_import(&mut self, name: &str, bytes: &[u8], shared: bool) -> Mem {
        self.memories.push(MemoryBytes::new(name, bytes));
        self.program.import_memory(MemoryImport {
            module: "test".into(),
            name: name.into(),
            minimum: 1,
            maximum: shared.then_some(1),
            shared,
        })
    }

    pub fn callback(&mut self, name: &str, signature: Signature, results: &[Value]) -> Func {
        self.callbacks.push(Callback::new(name, results));
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
        results: &[Type],
        build: impl FnOnce(BlockBuilder<'_>) -> Result<(), BuildError> + Send + 'static,
    ) -> TestModule {
        let run = self
            .program
            .raw_function(signature(parameters, results), build);
        self.finish(run)
    }

    pub fn expression<T: ValueType>(
        self,
        parameters: &[Type],
        build: impl FnOnce(&BlockBuilder<'_>) -> Val<T> + Send + 'static,
    ) -> TestModule {
        self.function(parameters, &[T::TYPE], move |body| {
            let value = build(&body);
            body.return_(value)
        })
    }

    /// Complete a fixture whose bodies need explicit declarations or multiple functions.
    pub fn finish(mut self, run: impl Into<Func>) -> TestModule {
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

thread_local! {
    static CAPTURED: std::cell::RefCell<Option<Box<dyn std::any::Any>>> = const {
        std::cell::RefCell::new(None)
    };
}

/// Retains a value on the calling thread through the public serial compilation API.
pub(crate) fn capture_from_body<T: 'static>(
    build: impl FnOnce(&mut BlockBuilder<'_>) -> T + Send + 'static,
) -> T {
    let mut program = Program::new();
    let function = program.raw_function(
        Signature {
            parameters: vec![Type::I32],
            results: vec![],
        },
        move |mut body| {
            let captured = build(&mut body);
            CAPTURED.with_borrow_mut(|slot| *slot = Some(Box::new(captured)));
            body.return_(())
        },
    );
    program.export("capture", function).unwrap();
    program.compile().unwrap();
    CAPTURED.with_borrow_mut(|slot| *slot.take().unwrap().downcast::<T>().ok().unwrap())
}
