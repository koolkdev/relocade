use crate::{
    Error, Import, ImportValue, MemoryAccess, ModuleCache, Result, WasmBackend, WasmCompiler,
    WasmMemoryAccess,
};
use js_sys::{Function as JsFunction, Object, Reflect, Uint8Array, WebAssembly};
use relocade_compiler::abi;
use std::{
    cell::{Ref, RefCell, RefMut},
    ops::{Deref, DerefMut},
    rc::Rc,
};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

mod calls;
mod context;
mod host;
pub(crate) mod memory;
pub use context::Context;
pub(crate) use memory::MemoryRegion;
pub use memory::{Memory, SharedMemory};

/// One guest's instances, callbacks and host state.
pub struct Backend<T = ()> {
    store: Rc<RefCell<Store<T>>>,
    modules: Rc<ModuleCache<Module>>,
}

struct Store<T> {
    state: T,
    memories: Vec<Rc<memory::Allocation>>,
}

impl<T> Store<T> {
    fn memory(&self, memory: &Memory) -> &Rc<memory::Allocation> {
        &self.memories[memory.0]
    }

    fn insert_memory(&mut self, allocation: Rc<memory::Allocation>) -> Memory {
        let memory = Memory(self.memories.len());
        self.memories.push(allocation);
        memory
    }
}

/// Compiled code whose JavaScript handle can be posted to another worker.
/// Equality compares handle identity, including across clones of this value.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Module(#[serde(with = "serde_wasm_bindgen::preserve")] WebAssembly::Module);

#[cfg(feature = "worker")]
impl web_thread::Post for Module {}

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

impl Backend {
    pub fn new() -> Result<Self> {
        Self::with_state(())
    }
}

impl<T: 'static> Backend<T> {
    pub fn with_state(state: T) -> Result<Self> {
        thread_local! {
            static MODULES: Rc<ModuleCache<Module>> =
                Rc::new(ModuleCache::default());
        }
        Ok(Self {
            store: Rc::new(RefCell::new(Store {
                state,
                memories: Vec::new(),
            })),
            modules: MODULES.with(Rc::clone),
        })
    }
}

impl<T: 'static> WasmBackend for Backend<T> {
    type State = T;
    type Context<'a> = Context<'a, T>;
    type Module = Module;
    type Compiler = Compiler;
    type Instance = WebAssembly::Instance;
    type Table = WebAssembly::Table;
    type ImportFunction = JsFunction;
    type SharedMemory = SharedMemory;

    fn state(&self) -> impl Deref<Target = T> {
        Ref::map(self.store.borrow(), |store| &store.state)
    }

    fn state_mut(&mut self) -> impl DerefMut<Target = T> {
        RefMut::map(self.store.borrow_mut(), |store| &mut store.state)
    }

    fn context(&mut self) -> Context<'_, T> {
        Context(self.store.borrow_mut())
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
        Ok(self.store.borrow_mut().insert_memory(allocation))
    }

    fn shared_memory(&self, memory: &Memory) -> Option<SharedMemory> {
        let store = self.store.borrow();
        let allocation = store.memory(memory);
        allocation
            .memory
            .buffer()
            .is_instance_of::<js_sys::SharedArrayBuffer>()
            .then(|| SharedMemory {
                allocation: Rc::clone(allocation),
            })
    }

    fn import_shared_memory(&mut self, memory: &SharedMemory) -> Memory {
        self.store
            .borrow_mut()
            .insert_memory(Rc::clone(&memory.allocation))
    }

    fn create_table(&mut self, minimum: u32) -> Result<Self::Table> {
        let descriptor = Object::new();
        set(&descriptor, "initial", &minimum.into())?;
        set(&descriptor, "element", &"anyfunc".into())?;
        WebAssembly::Table::new(&descriptor).map_err(|error| js_error("create Wasm table", error))
    }

    fn ensure_table_size(&mut self, table: &Self::Table, minimum: u32) -> Result<()> {
        let current = table.length();
        if minimum > current {
            table
                .grow(minimum - current)
                .map_err(|error| js_error("grow Wasm table", error))?;
        }
        Ok(())
    }

    fn ensure_memory_size(&mut self, memory: &Self::Memory, byte_len: u64) -> Result<()> {
        let store = self.store.borrow();
        let memory = &store.memory(memory).memory;
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

    async fn instantiate(
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
                ImportValue::Memory(memory) => {
                    self.store.borrow().memory(memory).memory.clone().into()
                }
                ImportValue::Table(table) => (*table).clone().into(),
                ImportValue::Function(function) => function.clone().into(),
            };
            set(&namespace, import.name(), &value)?;
        }
        JsFuture::from(WebAssembly::instantiate_module(&module.0, &bindings))
            .await
            .map(|instance| instance.unchecked_into())
            .map_err(|error| js_error("instantiate Wasm", error))
    }

    fn set_table_import(
        &mut self,
        table: &Self::Table,
        index: u32,
        function: JsFunction,
    ) -> Result<()> {
        table
            .set(index, &function)
            .map_err(|error| js_error("set Wasm table function", error))
    }
}

impl<T> WasmMemoryAccess for Backend<T> {
    type Memory = Memory;

    fn memory<'a>(&'a mut self, memory: &'a Memory) -> MemoryAccess<'a> {
        MemoryAccess::new(MemoryRegion::Stored(Ref::map(
            self.store.borrow(),
            |store| &store.memory(memory).transfers,
        )))
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
