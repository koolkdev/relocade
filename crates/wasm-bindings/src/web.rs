use crate::{lift_flat, lower_flat, slots::Tuple, AbiValue, Flat, Value, WasmRepr};

mod adapters;
mod bridge;
mod error;
mod returns;

pub use error::{Error, Result};
use js_sys::{Function as JsFunction, Object, Reflect, WebAssembly};
use wasm_bindgen::{JsCast, JsValue};

/// A tuple of zero through twelve Wasm scalars: `i32`, `i64`, `f32` or `f64`.
/// This trait is sealed; the binding implementation owns the Rust calling ABI.
#[allow(private_bounds)]
pub trait WasmTuple: bridge::Parameters + returns::Returns {}
impl<T: bridge::Parameters + returns::Returns> WasmTuple for T {}

/// An owned Wasm function with typed Rust arguments and results.
/// Its table entry and original export stay alive until this handle is dropped.
pub struct Function<
    P: AbiValue<Logical: WasmRepr<Slots: Tuple>>,
    R: AbiValue<Logical: WasmRepr<Slots: Tuple>>,
> {
    target: JsFunction,
    entry: bridge::Entry<Flat<P>, Flat<R>>,
}

impl<
        P: AbiValue<Logical: WasmRepr<Slots: Tuple>>,
        R: AbiValue<Logical: WasmRepr<Slots: Tuple>>,
    > Function<P, R>
where
    Flat<P>: WasmTuple,
    Flat<R>: WasmTuple,
{
    /// Binds a function, checking its Wasm signature during adapter linking.
    pub fn new(target: &JsFunction) -> Result<Self> {
        Ok(Self {
            target: target.clone(),
            entry: bridge::Entry::new(target)?,
        })
    }

    /// Resolves and binds a named export from an instance.
    pub fn from_export(instance: &WebAssembly::Instance, name: &str) -> Result<Self> {
        let target = Reflect::get(&instance.exports(), &name.into())
            .and_then(|value| value.dyn_into::<JsFunction>())
            .map_err(|error| js_error("resolve Wasm function", error))?;
        Self::new(&target)
    }

    /// Calls the function directly through its owned Rust-table entry.
    pub fn call(&self, arguments: Value<P>) -> Value<R> {
        lift_flat::<R>(self.entry.call(lower_flat::<P>(arguments))).expect("invalid Wasm ABI value")
    }

    /// The original function for imports and funcref tables.
    /// It retains the Wasm result signature, including multiple results.
    pub fn as_function(&self) -> &JsFunction {
        &self.target
    }
}

/// Wraps a Rust closure as an actual Wasm function for imports and tables.
/// Engine references keep the closure alive, including while it is executing.
pub fn callback<P, R>(invoke: impl Fn(Value<P>) -> Value<R> + 'static) -> Result<JsFunction>
where
    P: AbiValue<Logical: WasmRepr<Slots: Tuple>>,
    R: AbiValue<Logical: WasmRepr<Slots: Tuple>>,
    Flat<P>: WasmTuple,
    Flat<R>: WasmTuple,
{
    bridge::callback(move |arguments| {
        let arguments = lift_flat::<P>(arguments).expect("invalid Wasm ABI value");
        lower_flat::<R>(invoke(arguments))
    })
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
