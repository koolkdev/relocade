//! Browser implementation of the runtime's Wasm engine interface.
//!
//! Compilation and instantiation use the asynchronous WebAssembly APIs. Guest
//! calls are synchronous. Callbacks borrow host state only while running and
//! access guest memory through its current buffer, including after memory growth.
#![forbid(unsafe_code)]
#![cfg(target_arch = "wasm32")]

use js_sys::{Function as JsFunction, Object, Reflect, Uint8Array, WebAssembly};
use relocade_compiler::{abi::Function as FunctionDeclaration, I64};
use relocade_runtime::{
    wasm::{Import, ImportValue, WasmBackend, WasmMemoryAccess},
    Error, Result,
};
use std::{
    any::Any,
    cell::{Ref, RefCell, RefMut},
    ops::{Deref, DerefMut},
    rc::Rc,
};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

mod host;
pub use host::Context;

/// Modules, callbacks and host state belonging to one guest.
pub struct WebBackend<T = ()> {
    store: Rc<RefCell<Store<T>>>,
    // Rust must retain the closures for as long as guest modules can call them.
    callbacks: Vec<Box<dyn Any>>,
}

struct Store<T> {
    state: T,
    memories: Vec<WebAssembly::Memory>,
}

impl<T> Store<T> {
    fn memory(&self, memory: &Memory) -> &WebAssembly::Memory {
        &self.memories[memory.0]
    }
}

/// A handle to memory owned by one backend. Copies identify the same JavaScript
/// resource; access requires the originating backend or its scoped context.
#[derive(Clone, Copy, Debug)]
pub struct Memory(usize);

/// A resolved `() -> i64` export, kept distinct from raw host function handles.
pub struct Function(JsFunction);

impl WebBackend {
    pub fn new() -> Self {
        Self::with_state(())
    }
}

impl Default for WebBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: 'static> WebBackend<T> {
    pub fn with_state(state: T) -> Self {
        Self {
            store: Rc::new(RefCell::new(Store {
                state,
                memories: Vec::new(),
            })),
            callbacks: Vec::new(),
        }
    }
}

impl<T: 'static> WasmBackend for WebBackend<T> {
    type State = T;
    type Context<'a> = Context<'a, T>;
    type Module = WebAssembly::Module;
    type Instance = WebAssembly::Instance;
    type Table = WebAssembly::Table;
    type Function = Function;
    type ImportFunction = JsFunction;

    fn state(&self) -> impl Deref<Target = T> {
        Ref::map(self.store.borrow(), |store| &store.state)
    }

    fn state_mut(&mut self) -> impl DerefMut<Target = T> {
        RefMut::map(self.store.borrow_mut(), |store| &mut store.state)
    }

    fn context(&mut self) -> Context<'_, T> {
        Context(self.store.borrow_mut())
    }

    async fn compile(&self, bytes: &[u8]) -> Result<Self::Module> {
        let bytes = Uint8Array::from(bytes);
        JsFuture::from(WebAssembly::compile(&bytes))
            .await
            .map(|module| module.unchecked_into())
            .map_err(|error| js_error("compile Wasm", error))
    }

    fn create_memory(&mut self, minimum: u32, maximum: Option<u32>) -> Result<Self::Memory> {
        let descriptor = Object::new();
        set(&descriptor, "initial", &minimum.into())?;
        if let Some(maximum) = maximum {
            set(&descriptor, "maximum", &maximum.into())?;
        }
        let memory = WebAssembly::Memory::new(&descriptor)
            .map_err(|error| js_error("create Wasm memory", error))?;
        let mut store = self.store.borrow_mut();
        let handle = Memory(store.memories.len());
        store.memories.push(memory);
        Ok(handle)
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
        let memory = store.memory(memory);
        let pages = byte_len.div_ceil(65536);
        let byte_length = Reflect::get(&memory.buffer(), &"byteLength".into())
            .map_err(|error| js_error("read Wasm memory size", error))?
            .as_f64()
            .expect("ArrayBuffer byteLength is a number") as u64;
        let current = byte_length / 65536;
        if pages > current {
            // js-sys's Memory::grow binding does not catch allocation failures.
            let grow = Reflect::get(memory, &"grow".into())
                .map_err(|error| js_error("get Wasm memory grow", error))?
                .unchecked_into::<JsFunction>();
            grow.call1(memory, &JsValue::from_f64((pages - current) as f64))
                .map_err(|error| js_error("grow Wasm memory", error))?;
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
                ImportValue::Memory(memory) => self.store.borrow().memory(memory).clone().into(),
                ImportValue::Table(table) => (*table).clone().into(),
                ImportValue::Function(function) => function.clone().into(),
            };
            set(&namespace, import.name(), &value)?;
        }
        JsFuture::from(WebAssembly::instantiate_module(module, &bindings))
            .await
            .map(|instance| instance.unchecked_into())
            .map_err(|error| js_error("instantiate Wasm", error))
    }

    fn function(
        &mut self,
        instance: &Self::Instance,
        declaration: FunctionDeclaration<fn() -> I64>,
    ) -> Result<Function> {
        Reflect::get(&instance.exports(), &declaration.name.into())
            .and_then(|value| value.dyn_into::<JsFunction>())
            .map(Function)
            .map_err(|error| js_error("resolve Wasm function", error))
    }

    fn import_function(&self, function: &Function) -> JsFunction {
        function.0.clone()
    }

    fn set_table_function(
        &mut self,
        table: &Self::Table,
        index: u32,
        function: &Function,
    ) -> Result<()> {
        table
            .set(index, &function.0)
            .map_err(|error| js_error("set Wasm table function", error))
    }

    fn call(&mut self, function: &Function) -> Result<i64> {
        let value = function
            .0
            .call0(&JsValue::UNDEFINED)
            .map_err(|error| js_error("call Wasm function", error))?;
        i64::try_from(value).map_err(|error| js_error("read Wasm i64 result", error))
    }
}

impl<T> WasmMemoryAccess for WebBackend<T> {
    type Memory = Memory;

    fn read_memory(&self, memory: &Self::Memory, offset: u32, bytes: &mut [u8]) {
        self.store.borrow().read_memory(memory, offset, bytes);
    }

    fn write_memory(&mut self, memory: &Self::Memory, offset: u32, bytes: &[u8]) {
        self.store.borrow_mut().write_memory(memory, offset, bytes);
    }
}

impl<T> WasmMemoryAccess for Store<T> {
    type Memory = Memory;

    fn read_memory(&self, memory: &Memory, offset: u32, bytes: &mut [u8]) {
        memory_range(self.memory(memory), offset, bytes.len()).copy_to(bytes);
    }

    fn write_memory(&mut self, memory: &Memory, offset: u32, bytes: &[u8]) {
        memory_range(self.memory(memory), offset, bytes.len()).copy_from(bytes);
    }
}

fn memory_range(memory: &WebAssembly::Memory, offset: u32, len: usize) -> Uint8Array {
    // A memory.grow detaches the old buffer, so obtain it for each transfer.
    Uint8Array::new_with_byte_offset_and_length(
        &memory.buffer(),
        offset,
        len.try_into().expect("Wasm32 memory range"),
    )
}

fn set(object: &Object, name: &str, value: &JsValue) -> Result<()> {
    Reflect::set(object, &name.into(), value)
        .map(|_| ())
        .map_err(|error| js_error("bind Wasm import", error))
}

fn js_error(operation: &'static str, error: JsValue) -> Error {
    Error::wasm(
        operation,
        error.as_string().unwrap_or_else(|| format!("{error:?}")),
    )
}
