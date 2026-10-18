//! Browser implementation of the runtime's Wasm engine interface.
//!
//! Compilation and instantiation use the asynchronous WebAssembly APIs. Guest
//! calls are synchronous. Callbacks borrow host state only while running and
//! access guest memory through its current buffer, including after memory growth.
#![forbid(unsafe_code)]
#![cfg(target_arch = "wasm32")]

use js_sys::{Function as JsFunction, Object, Reflect, Uint8Array, WebAssembly};
use std::{
    any::Any,
    cell::RefCell,
    ops::{Deref, DerefMut},
    rc::Rc,
};
use wasm86_compiler::{abi::Function as FunctionDeclaration, Results, ValueType, I64};
use wasm86_runtime::{
    wasm::{
        HostFunction, HostValue, Import, ImportValue, WasmBackend, WasmCaller, WasmMemoryAccess,
    },
    Error, Result,
};
use wasm_bindgen::{
    closure::Closure,
    convert::{FromWasmAbi, ReturnWasmAbi},
    JsCast, JsValue,
};
use wasm_bindgen_futures::JsFuture;

/// Modules, callbacks and host state belonging to one guest.
pub struct WebBackend<T = ()> {
    state: Rc<RefCell<T>>,
    // Rust must retain the closures for as long as guest modules can call them.
    callbacks: Vec<Box<dyn Any>>,
}

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
            state: Rc::new(RefCell::new(state)),
            callbacks: Vec::new(),
        }
    }
}

impl<T: 'static> WasmBackend for WebBackend<T> {
    type State = T;
    type Caller<'a> = Caller<'a, T>;
    type Module = WebAssembly::Module;
    type Instance = WebAssembly::Instance;
    type Table = WebAssembly::Table;
    type Function = Function;
    type ImportFunction = JsFunction;

    fn state(&self) -> impl Deref<Target = T> {
        self.state.borrow()
    }

    fn state_mut(&mut self) -> impl DerefMut<Target = T> {
        self.state.borrow_mut()
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
        WebAssembly::Memory::new(&descriptor).map_err(|error| js_error("create Wasm memory", error))
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
                ImportValue::Memory(memory) => (*memory).clone().into(),
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
    type Memory = WebAssembly::Memory;

    fn read_memory(&self, memory: &Self::Memory, offset: u32, bytes: &mut [u8]) {
        memory_range(memory, offset, bytes.len()).copy_to(bytes);
    }

    fn write_memory(&mut self, memory: &Self::Memory, offset: u32, bytes: &[u8]) {
        memory_range(memory, offset, bytes.len()).copy_from(bytes);
    }
}

macro_rules! host_function {
    ($($parameter:ident : $argument:ident),*) => {
        impl<T, R, $($parameter),*> HostFunction<fn($($parameter),*) -> R> for WebBackend<T>
        where
            T: 'static,
            R: Results + HostValue,
            R::Carrier: ReturnWasmAbi,
            $($parameter: ValueType + HostValue, $parameter::Carrier: FromWasmAbi),*
        {
            fn host_function(
                &mut self,
                callback: fn(Caller<'_, T>, $($parameter::Carrier),*) -> R::Carrier,
            ) -> JsFunction {
                let state = Rc::clone(&self.state);
                let closure = Closure::<dyn FnMut($($parameter::Carrier),*) -> R::Carrier>::new(move |$($argument),*| {
                    callback(Caller(&mut *state.borrow_mut()), $($argument),*)
                });
                let function = closure.as_ref().unchecked_ref::<JsFunction>().clone();
                self.callbacks.push(Box::new(closure));
                function
            }
        }
    };
}

host_function!();
host_function!(A: a);
host_function!(A: a, B: b);
host_function!(A: a, B: b, C: c);
host_function!(A: a, B: b, C: c, D: d);

/// A callback's scoped borrow of host state. Memory stays in the browser engine.
pub struct Caller<'a, T>(&'a mut T);

impl<T> WasmCaller for Caller<'_, T> {
    type State = T;

    fn state(&self) -> &T {
        self.0
    }
    fn state_mut(&mut self) -> &mut T {
        self.0
    }
}

impl<T> WasmMemoryAccess for Caller<'_, T> {
    type Memory = WebAssembly::Memory;

    fn read_memory(&self, memory: &Self::Memory, offset: u32, bytes: &mut [u8]) {
        memory_range(memory, offset, bytes.len()).copy_to(bytes);
    }
    fn write_memory(&mut self, memory: &Self::Memory, offset: u32, bytes: &[u8]) {
        memory_range(memory, offset, bytes.len()).copy_from(bytes);
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
