//! Links direct Wasm functions using their scalar signatures.
//!
//! Parameters and scalar results cross unchanged. Only multiple results require
//! glue for Rust's C ABI, which uses a hidden pointer to a repr(C) return value.

mod encode;

use super::{
    js_error,
    returns::{Area, Layout},
    set,
};
use crate::Result;
use js_sys::{Function as JsFunction, Object, Reflect, SharedArrayBuffer, Uint8Array, WebAssembly};
use std::{cell::RefCell, collections::HashMap};
use wasm_bindgen::{JsCast, JsValue};
use wasm_encoder::ValType;

#[derive(Clone, Copy, Hash, PartialEq, Eq)]
enum Direction {
    Export,
    Callback,
}

#[derive(Hash, PartialEq, Eq)]
struct Shape {
    parameters: Vec<ValType>,
    results: Vec<ValType>,
    area: Option<Area>,
    direction: Direction,
    shared: bool,
}

fn instantiate(
    parameters: Vec<ValType>,
    returns: Layout,
    direction: Direction,
    env: Object,
) -> Result<JsFunction> {
    let shared = if returns.area.is_some() {
        let memory = wasm_bindgen::memory().unchecked_into::<WebAssembly::Memory>();
        set(&env, "memory", &memory)?;
        memory.buffer().is_instance_of::<SharedArrayBuffer>()
    } else {
        false
    };
    let shape = Shape {
        parameters,
        results: returns.types,
        area: returns.area,
        direction,
        shared,
    };
    thread_local! {
        static MODULES: RefCell<HashMap<Shape, WebAssembly::Module>> = RefCell::default();
    }
    let cached = MODULES.with(|modules| modules.borrow().get(&shape).cloned());
    let module = match cached {
        Some(module) => module,
        None => {
            let bytes = Uint8Array::from(encode::module(&shape).as_slice());
            let module = WebAssembly::Module::new(&bytes)
                .map_err(|error| js_error("compile Wasm adapter", error))?;
            MODULES.with(|modules| modules.borrow_mut().insert(shape, module.clone()));
            module
        }
    };
    let imports = Object::new();
    set(&imports, "env", &env)?;
    let instance = WebAssembly::Instance::new(&module, &imports)
        .map_err(|error| js_error("link Wasm adapter", error))?;
    Reflect::get(&instance.exports(), &"run".into())
        .and_then(|value| value.dyn_into())
        .map_err(|error| js_error("resolve Wasm adapter", error))
}

pub(super) fn export(
    parameters: Vec<ValType>,
    returns: Layout,
    target: &JsFunction,
) -> Result<JsFunction> {
    let env = Object::new();
    set(&env, "target", target)?;
    instantiate(parameters, returns, Direction::Export, env)
}

pub(super) fn callback(
    parameters: Vec<ValType>,
    returns: Layout,
    target: &JsFunction,
    anchor: &JsValue,
    context: u32,
) -> Result<JsFunction> {
    let env = Object::new();
    set(&env, "target", target)?;
    let descriptor = Object::new();
    set(&descriptor, "value", &"externref".into())?;
    set(&descriptor, "mutable", &true.into())?;
    let anchor = WebAssembly::Global::new(&descriptor, anchor)
        .map_err(|error| js_error("create callback owner", error))?;
    set(&env, "anchor", &anchor)?;
    set(&env, "context", &context.into())?;
    if returns.area.is_some() {
        let stack = Reflect::get(&wasm_bindgen::exports(), &"__stack_pointer".into())
            .map_err(|error| js_error("resolve Rust stack pointer", error))?;
        set(&env, "stack", &stack)?;
    }
    instantiate(parameters, returns, Direction::Callback, env)
}
