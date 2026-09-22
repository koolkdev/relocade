use std::path::Path;

use serde::{Deserialize, Serialize};
use wasmtime::{
    AsContext, ExternType, Func, Linker, Memory, MemoryType, Store, Val, ValType, WasmParams,
    WasmResults,
};

pub use wasm86_test_support::Value;
use wasm86_test_support::{engine, Module, Outcome, SharedBytes};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryBytes {
    pub name: String,
    pub bytes: Vec<u8>,
}

impl MemoryBytes {
    pub fn new(name: &str, bytes: &[u8]) -> Self {
        Self {
            name: name.into(),
            bytes: bytes.into(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Callback {
    name: String,
    results: Vec<Value>,
}

impl Callback {
    pub fn new(name: &str, results: &[Value]) -> Self {
        Self {
            name: name.into(),
            results: results.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Call {
    pub name: String,
    pub arguments: Vec<Value>,
    pub memories: Vec<MemoryBytes>,
}

impl Call {
    pub fn new(name: &str, arguments: &[Value]) -> Self {
        Self {
            name: name.into(),
            arguments: arguments.into(),
            memories: vec![],
        }
    }

    pub fn with_memories(mut self, memories: &[MemoryBytes]) -> Self {
        self.memories = memories.into();
        self
    }
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Observation {
    pub outcome: Outcome,
    pub callbacks: Vec<Call>,
    pub memories: Vec<MemoryBytes>,
}

impl Observation {
    pub fn returned(values: &[Value]) -> Self {
        Self {
            outcome: Outcome::Returned(values.into()),
            callbacks: vec![],
            memories: vec![],
        }
    }

    pub fn with_callbacks(mut self, callbacks: &[Call]) -> Self {
        self.callbacks = callbacks.into();
        self
    }

    pub fn with_memories(mut self, memories: &[MemoryBytes]) -> Self {
        self.memories = memories.into();
        self
    }
}

#[derive(Debug, Serialize)]
pub struct Input {
    entry: String,
    arguments: Vec<Value>,
    memories: Vec<MemoryBytes>,
    callbacks: Vec<Callback>,
}

impl Input {
    pub fn call(entry: &str, arguments: &[Value]) -> Self {
        Self {
            entry: entry.into(),
            arguments: arguments.into(),
            memories: vec![],
            callbacks: vec![],
        }
    }

    pub fn with_memories(mut self, memories: &[MemoryBytes]) -> Self {
        self.memories = memories.into();
        self
    }

    pub fn with_callbacks(mut self, callbacks: &[Callback]) -> Self {
        self.callbacks = callbacks.into();
        self
    }
}

pub struct TestModule {
    module: Module,
    memories: Vec<MemoryBytes>,
    callbacks: Vec<Callback>,
}

impl TestModule {
    pub fn new(bytes: &[u8]) -> Self {
        Self::with_imports(bytes, vec![], vec![])
    }

    pub fn with_imports(
        bytes: &[u8],
        memories: Vec<MemoryBytes>,
        callbacks: Vec<Callback>,
    ) -> Self {
        Self {
            module: Module::new(bytes),
            memories,
            callbacks,
        }
    }

    pub fn bytes(&self) -> &[u8] {
        self.module.bytes()
    }

    pub fn run_v8(&self, input: &Input) -> Observation {
        #[derive(Serialize)]
        struct MemoryDescriptor<'a> {
            name: &'a str,
            initial: u64,
            #[serde(skip_serializing_if = "Option::is_none")]
            maximum: Option<u64>,
            shared: bool,
        }
        #[derive(Serialize)]
        struct V8Input<'a> {
            #[serde(flatten)]
            call: &'a Input,
            memory_imports: Vec<MemoryDescriptor<'a>>,
        }
        let mut memory_imports = Vec::new();
        for payload in wasmparser::Parser::new(0).parse_all(self.bytes()) {
            if let wasmparser::Payload::ImportSection(imports) = payload.unwrap() {
                for import in imports {
                    let import = import.unwrap();
                    if let wasmparser::TypeRef::Memory(memory) = import.ty {
                        memory_imports.push(MemoryDescriptor {
                            name: import.name,
                            initial: memory.initial,
                            maximum: memory.maximum,
                            shared: memory.shared,
                        });
                    }
                }
            }
        }
        self.module.run_v8(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/execute.mjs"),
            &V8Input {
                call: input,
                memory_imports,
            },
        )
    }

    pub fn instantiate(&self) -> Instance {
        self.instantiate_with_shared(&[])
    }

    /// Reuses already initialized shared memory across independent instances.
    /// Supplied memories retain their contents instead of applying fixture bytes.
    pub fn instantiate_with_shared(&self, shared: &[(&str, SharedBytes)]) -> Instance {
        let engine = engine();
        let module = self.module.wasmtime();
        let mut store = Store::new(engine, Vec::<Call>::new());
        let mut linker = Linker::new(engine);
        let memories = self
            .memories
            .iter()
            .map(|initial| {
                let memory = if let Some((_, memory)) =
                    shared.iter().find(|(name, _)| *name == initial.name)
                {
                    MemoryBinding::Shared(memory.clone())
                } else {
                    let memory_type = module
                        .imports()
                        .find(|import| import.module() == "test" && import.name() == initial.name)
                        .and_then(|import| match import.ty() {
                            ExternType::Memory(memory) => Some(memory),
                            _ => None,
                        })
                        .unwrap_or_else(|| MemoryType::new(1, None));
                    if memory_type.is_shared() {
                        let memory = SharedBytes::new(
                            memory_type.minimum() as u32,
                            memory_type.maximum().unwrap() as u32,
                        );
                        memory.write(0, &initial.bytes);
                        MemoryBinding::Shared(memory)
                    } else {
                        let memory =
                            Memory::new(&mut store, memory_type).expect("create test memory");
                        memory
                            .write(&mut store, 0, &initial.bytes)
                            .expect("initialize test memory");
                        MemoryBinding::Private(memory)
                    }
                };
                linker
                    .define(&store, "test", &initial.name, memory.external())
                    .expect("define test memory");
                (initial.name.clone(), memory, initial.bytes.len())
            })
            .collect::<Vec<_>>();
        for callback in &self.callbacks {
            let Some(import) = module
                .imports()
                .find(|import| import.module() == "test" && import.name() == callback.name)
            else {
                continue;
            };
            let ExternType::Func(ty) = import.ty() else {
                panic!("callback import must be a function")
            };
            assert!(
                callback.results.len() == ty.results().len()
                    && callback
                        .results
                        .iter()
                        .zip(ty.results())
                        .all(|(value, ty)| {
                            matches!(
                                (value, ty),
                                (Value::I32(_), ValType::I32) | (Value::I64(_), ValType::I64)
                            )
                        }),
                "callback result does not match its import signature"
            );
            let callback = callback.clone();
            let name = callback.name.clone();
            let memories = memories.clone();
            let function = Func::new(&mut store, ty, move |mut caller, arguments, results| {
                let call = Call {
                    name: callback.name.clone(),
                    arguments: arguments.iter().map(Value::from_wasm).collect(),
                    memories: snapshot(&caller, &memories),
                };
                caller.data_mut().push(call);
                for (result, value) in results.iter_mut().zip(&callback.results) {
                    *result = value.wasm();
                }
                Ok(())
            });
            linker
                .define(&store, "test", &name, function)
                .expect("define test callback");
        }
        let instance = linker
            .instantiate(&mut store, module)
            .expect("instantiate test module");
        Instance {
            store,
            instance,
            memories,
        }
    }
}

pub struct Instance {
    store: Store<Vec<Call>>,
    instance: wasmtime::Instance,
    memories: Vec<(String, MemoryBinding, usize)>,
}

impl Instance {
    pub fn call<R: WasmResults>(
        &mut self,
        arguments: impl WasmParams,
    ) -> Result<R, wasmtime::Trap> {
        self.call_export("run", arguments)
    }

    pub fn call_export<R: WasmResults>(
        &mut self,
        name: &str,
        arguments: impl WasmParams,
    ) -> Result<R, wasmtime::Trap> {
        let function = self
            .instance
            .get_typed_func::<_, R>(&mut self.store, name)
            .unwrap_or_else(|error| panic!("resolve fixture export {name}: {error:#}"));
        function.call(&mut self.store, arguments).map_err(|error| {
            error
                .downcast::<wasmtime::Trap>()
                .unwrap_or_else(|error| panic!("invoke fixture export {name}: {error:#}"))
        })
    }

    pub fn memory(&self, name: &str) -> Vec<u8> {
        let (_, memory, _) = self
            .memories
            .iter()
            .find(|(memory_name, _, _)| memory_name == name)
            .expect("fixture memory must exist");
        memory.read(&self.store, memory.size(&self.store))
    }

    pub fn callbacks(&self) -> &[Call] {
        self.store.data()
    }

    pub fn call_values(
        &mut self,
        entry: &str,
        arguments: &[Value],
    ) -> Result<Vec<Value>, wasmtime::Trap> {
        let function = self
            .instance
            .get_func(&mut self.store, entry)
            .expect("test export must exist");
        let arguments = arguments
            .iter()
            .map(|value| value.wasm())
            .collect::<Vec<_>>();
        let mut results = function
            .ty(&self.store)
            .results()
            .map(|ty| match ty {
                ValType::I32 => Val::I32(0),
                ValType::I64 => Val::I64(0),
                _ => panic!("test exports return only integers"),
            })
            .collect::<Vec<_>>();
        function
            .call(&mut self.store, &arguments, &mut results)
            .map_err(|error| {
                error
                    .downcast::<wasmtime::Trap>()
                    .unwrap_or_else(|error| panic!("invoke fixture export {entry}: {error:#}"))
            })?;
        Ok(results.iter().map(Value::from_wasm).collect())
    }
}

#[derive(Clone)]
enum MemoryBinding {
    Private(Memory),
    Shared(SharedBytes),
}

impl MemoryBinding {
    fn external(&self) -> wasmtime::Extern {
        match self {
            Self::Private(memory) => (*memory).into(),
            Self::Shared(memory) => memory.memory().clone().into(),
        }
    }

    fn size(&self, store: impl AsContext) -> usize {
        match self {
            Self::Private(memory) => memory.data_size(store),
            Self::Shared(memory) => memory.memory().data_size(),
        }
    }

    fn read(&self, store: impl AsContext, length: usize) -> Vec<u8> {
        match self {
            Self::Private(memory) => memory.data(store.as_context())[..length].to_vec(),
            Self::Shared(memory) => memory.read(0, length),
        }
    }
}

fn snapshot(
    store: impl AsContext,
    memories: &[(String, MemoryBinding, usize)],
) -> Vec<MemoryBytes> {
    memories
        .iter()
        .map(|(name, memory, length)| {
            MemoryBytes::new(name, &memory.read(store.as_context(), *length))
        })
        .collect()
}
