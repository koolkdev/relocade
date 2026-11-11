use crate::{
    Error, Import, ImportValue, MemoryAccess, ModuleCache, Result, WasmBackend, WasmCompiler,
    WasmMemoryAccess,
};
use js_sys::{Function as JsFunction, Object, Reflect, Uint8Array, WebAssembly};
use relocade_compiler::abi;
use std::rc::Rc;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

mod calls;
mod context;
mod host;
pub(crate) mod memory;
mod tables;
pub use context::Context;
pub(crate) use memory::MemoryRegion;
pub use memory::{Memory, SharedMemory};

/// Links and executes resources on one JavaScript thread.
pub struct WasmRuntime {
    modules: Rc<ModuleCache<Module>>,
}

/// Compiled code whose JavaScript handle can be posted to another worker.
/// Equality compares handle identity, including across clones of this value.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Module(#[serde(with = "serde_wasm_bindgen::preserve")] WebAssembly::Module);

impl crate::worker::Post for Module {}

/// A stateless compiler that uses the calling JavaScript thread's WebAssembly API.
#[derive(Clone)]
pub struct Compiler;

impl WasmCompiler for Compiler {
    type Module = Module;

    async fn compile(&self, bytes: &[u8]) -> Result<Module> {
        let bytes = Uint8Array::from(bytes);
        JsFuture::from(WebAssembly::compile(&bytes))
            .await
            .map(|module| Module(module.unchecked_into()))
            .map_err(|error| js_error("compile Wasm", error))
    }
}

impl WasmRuntime {
    pub fn new() -> Result<Self> {
        thread_local! {
            static MODULES: Rc<ModuleCache<Module>> =
                Rc::new(ModuleCache::default());
        }
        Ok(Self {
            modules: MODULES.with(Rc::clone),
        })
    }
}

impl WasmBackend for WasmRuntime {
    type Context<'a> = Context<'a>;
    type Module = Module;
    type Compiler = Compiler;
    type Instance = WebAssembly::Instance;
    type Table = WebAssembly::Table;
    type ImportFunction = JsFunction;
    type SharedMemory = SharedMemory;

    fn context(&mut self) -> Context<'_> {
        Context::new()
    }

    fn compiler(&self) -> Compiler {
        Compiler
    }

    fn module_cache(&self) -> Option<&ModuleCache<Self::Module>> {
        Some(&self.modules)
    }

    fn create_memory(&mut self, declaration: abi::Memory) -> Result<Self::Memory> {
        let memory =
            memory::allocate(declaration.minimum, declaration.maximum, declaration.shared)?;
        let allocation = Rc::new(memory::Allocation::new(memory, declaration.shared)?);
        Ok(Memory(allocation))
    }

    fn shared_memory(&self, memory: &Memory) -> Option<SharedMemory> {
        let allocation = &memory.0;
        allocation
            .memory
            .buffer()
            .is_instance_of::<js_sys::SharedArrayBuffer>()
            .then(|| SharedMemory {
                allocation: Rc::clone(allocation),
            })
    }

    fn import_shared_memory(&mut self, memory: &SharedMemory) -> Memory {
        Memory(Rc::clone(&memory.allocation))
    }

    fn allocate_table(
        &mut self,
        minimum: u32,
        maximum: Option<u32>,
        initial: Option<&JsFunction>,
    ) -> Result<Self::Table> {
        tables::create(minimum, maximum, initial)
    }

    fn ensure_memory_size(&mut self, memory: &Self::Memory, byte_len: u64) -> Result<()> {
        let memory = &memory.0.memory;
        let pages = byte_len.div_ceil(65536);
        let current = memory::size(memory)?;
        if pages > current {
            // js-sys's Memory::grow binding does not catch allocation failures.
            let grow = Reflect::get(memory, &"grow".into())
                .map_err(|error| js_error("get Wasm memory grow", error))?
                .unchecked_into::<JsFunction>();
            if let Err(error) = grow.call1(memory, &JsValue::from_f64((pages - current) as f64)) {
                // A worker may have satisfied the request since our size check.
                if memory::size(memory)? < pages {
                    return Err(js_error("grow Wasm memory", error));
                }
            }
        }
        Ok(())
    }

    fn instantiate(
        &mut self,
        module: &Self::Module,
        imports: &[Import<'_, Self>],
    ) -> Result<Self::Instance> {
        let bindings = Object::create(&JsValue::NULL.unchecked_into());
        for import in imports {
            let namespace = Reflect::get(&bindings, &import.module().into())
                .map_err(|error| js_error("read Wasm imports", error))?;
            let namespace = if namespace.is_undefined() {
                let namespace = Object::create(&JsValue::NULL.unchecked_into());
                set(&bindings, import.module(), &namespace)?;
                namespace
            } else {
                namespace.unchecked_into::<Object>()
            };
            let value: JsValue = match import.value() {
                ImportValue::Memory(memory) => memory.0.memory.clone().into(),
                ImportValue::Table(table) => (*table).clone().into(),
                ImportValue::Function(function) => function.clone().into(),
            };
            set(&namespace, import.name(), &value)?;
        }
        // Linking and module startup finish while this call borrows the runtime.
        WebAssembly::Instance::new(&module.0, &bindings)
            .map_err(|error| js_error("instantiate Wasm", error))
    }
}

impl WasmMemoryAccess for WasmRuntime {
    type Memory = Memory;

    fn memory<'a>(&'a mut self, memory: &'a Memory) -> MemoryAccess<'a> {
        memory.0.access()
    }
}

fn set(object: &Object, name: &str, value: &JsValue) -> Result<()> {
    Reflect::set(object, &name.into(), value)
        .map(|_| ())
        .map_err(|error| js_error("bind Wasm import", error))
}

fn js_error(operation: &'static str, error: JsValue) -> Error {
    Error::new(
        operation,
        error.as_string().unwrap_or_else(|| format!("{error:?}")),
    )
}
