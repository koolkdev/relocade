use std::{cell::OnceCell, path::Path};

use serde::{Deserialize, Serialize};
use wasmtime::{
    AsContext, ExternType, Func, Linker, Memory, MemoryType, Module, Store, Val, ValType,
    WasmParams, WasmResults,
};

pub use wasm86_test_support::Value;
use wasm86_test_support::{engine, run_v8, Outcome};

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
    result: Option<Value>,
}

impl Callback {
    pub fn new(name: &str, result: Value) -> Self {
        Self {
            name: name.into(),
            result: Some(result),
        }
    }

    pub fn void(name: &str) -> Self {
        Self {
            name: name.into(),
            result: None,
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
    pub fn returned(value: Value) -> Self {
        Self {
            outcome: Outcome::Returned(Some(value)),
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
    bytes: Vec<u8>,
    compiled: OnceCell<Module>,
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
            bytes: bytes.into(),
            compiled: OnceCell::new(),
            memories,
            callbacks,
        }
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn run_v8(&self, input: &Input) -> Observation {
        run_v8(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/execute.mjs"),
            &self.bytes,
            input,
        )
    }

    pub fn instantiate(&self) -> Instance {
        let engine = engine();
        let module = self
            .compiled
            .get_or_init(|| Module::new(engine, &self.bytes).expect("compile test module"));
        let mut store = Store::new(engine, Vec::<Call>::new());
        let mut linker = Linker::new(engine);
        let memories = self
            .memories
            .iter()
            .map(|initial| {
                let memory =
                    Memory::new(&mut store, MemoryType::new(1, None)).expect("create test memory");
                memory
                    .write(&mut store, 0, &initial.bytes)
                    .expect("initialize test memory");
                linker
                    .define(&store, "test", &initial.name, memory)
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
                matches!(
                    (callback.result, ty.results().collect::<Vec<_>>().as_slice()),
                    (None, [])
                        | (Some(Value::I32(_)), [ValType::I32])
                        | (Some(Value::I64(_)), [ValType::I64])
                ),
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
                if let Some(value) = callback.result {
                    results[0] = value.wasm();
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
    memories: Vec<(String, Memory, usize)>,
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

    pub fn memory(&self, name: &str) -> &[u8] {
        let (_, memory, _) = self
            .memories
            .iter()
            .find(|(memory_name, _, _)| memory_name == name)
            .expect("fixture memory must exist");
        memory.data(&self.store)
    }

    pub fn callbacks(&self) -> &[Call] {
        self.store.data()
    }

    pub fn call_values(
        &mut self,
        entry: &str,
        arguments: &[Value],
    ) -> Result<Option<Value>, wasmtime::Trap> {
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
        assert!(
            results.len() <= 1,
            "test exports return at most one integer"
        );
        function
            .call(&mut self.store, &arguments, &mut results)
            .map_err(|error| {
                error
                    .downcast::<wasmtime::Trap>()
                    .unwrap_or_else(|error| panic!("invoke fixture export {entry}: {error:#}"))
            })?;
        Ok(results.first().map(Value::from_wasm))
    }
}

fn snapshot(store: impl AsContext, memories: &[(String, Memory, usize)]) -> Vec<MemoryBytes> {
    memories
        .iter()
        .map(|(name, memory, length)| {
            MemoryBytes::new(name, &memory.data(store.as_context())[..*length])
        })
        .collect()
}
