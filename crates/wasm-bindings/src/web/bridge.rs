//! Typed Rust function pointers shared with generated Wasm adapters.
//!
//! Only this module converts Rust table indices and callback context pointers.
//! The private parameter/result traits are implemented for known scalar C types.
#![allow(unsafe_code)]

use super::{
    adapters,
    returns::{Returns, Scalar},
};
use crate::Result;
use js_sys::{Function as JsFunction, WebAssembly};
use std::{cell::RefCell, marker::PhantomData};
use wasm_bindgen::{prelude::*, JsCast};
use wasm_encoder::ValType;

struct Table {
    table: WebAssembly::Table,
    free: Vec<u32>,
}
thread_local! {
    static TABLE: RefCell<Table> = RefCell::new(Table {
        table: wasm_bindgen::function_table().unchecked_into(),
        free: Vec::new(),
    });
}

/// An owned Rust-table slot with the C ABI of P's scalars returning R::Abi.
pub(super) struct Entry<P, R> {
    index: u32,
    signature: PhantomData<fn(P) -> R>,
}

impl<P: Parameters, R: Returns> Entry<P, R> {
    pub(super) fn new(target: &JsFunction) -> Result<Self> {
        let adapter = adapters::export(P::types(), R::layout(), target)?;
        TABLE.with(|table| {
            let mut table = table.borrow_mut();
            let index = match table.free.pop() {
                Some(index) => index,
                None => table.table.grow(1).map_err(|error| {
                    super::js_error(
                        "grow Rust function table (link with --growable-table)",
                        error,
                    )
                })?,
            };
            if let Err(error) = table.table.set(index, &adapter) {
                table.free.push(index);
                return Err(super::js_error("install Wasm adapter", error));
            }
            Ok(Self {
                index,
                signature: PhantomData,
            })
        })
    }

    pub(super) fn call(&self, arguments: P) -> R {
        // This owned slot has the exact signature used to construct the adapter.
        unsafe { arguments.call::<R>(self.index) }
    }
}

impl<P, R> Drop for Entry<P, R> {
    fn drop(&mut self) {
        TABLE.with(|table| {
            let mut table = table.borrow_mut();
            table
                .table
                .set_raw(self.index, &JsValue::NULL)
                .expect("owned Rust table slot");
            table.free.push(self.index);
        });
    }
}

pub(super) fn callback<P: Parameters, R: Returns>(
    invoke: impl Fn(P) -> R + 'static,
) -> Result<JsFunction> {
    let invoke = Box::new(invoke);
    let context = (&*invoke as *const _) as u32;
    let index = P::trampoline::<R, _>(&*invoke);
    let target = TABLE
        .with(|table| table.borrow().table.get(index))
        .map_err(|error| super::js_error("resolve Rust callback trampoline", error))?;
    // This closure is only a GC ownership anchor. Its JavaScript wrapper never
    // handles a call; its finalizer drops the captured Rust callback allocation.
    let anchor = Closure::<dyn Fn()>::new(move || {
        std::hint::black_box(&invoke);
    })
    .into_js_value();
    adapters::callback(P::types(), R::layout(), &target, &anchor, context)
}

/// Keeps a callback owner rooted across its direct Wasm call. The generated
/// adapter pushes the externref before calling Rust and consumes it afterward.
#[wasm_bindgen]
pub fn __relocade_wasm_bindings_retain_callback(anchor: &JsValue) {
    std::hint::black_box(anchor);
}

pub(super) trait Parameters: Sized + 'static {
    fn types() -> Vec<ValType>;
    /// `index` must identify the C signature for these parameters and R::Abi.
    unsafe fn call<R: Returns>(self, index: u32) -> R;
    fn trampoline<R: Returns, Callback: Fn(Self) -> R>(_: &Callback) -> u32;
}

macro_rules! parameters {
    (@rest) => {};
    (@rest $first:ident $value:ident $(, $rest:ident $rest_value:ident)*) => {
        parameters!($($rest $rest_value),*);
    };
    ($($parameter:ident $argument:ident),*) => {
        impl<$($parameter: Scalar),*> Parameters for ($($parameter,)*) {
            fn types() -> Vec<ValType> { vec![$($parameter::TYPE),*] }
            unsafe fn call<R: Returns>(self, index: u32) -> R {
                // wasm32 function pointers are indices into the Rust table. R's
                // repr(C) aggregate lets Rust supply its own hidden return area.
                let invoke = unsafe { std::mem::transmute::<u32, unsafe extern "C" fn($($parameter),*) -> R::Abi>(index) };
                let ($($argument,)*) = self;
                R::from_abi(unsafe { invoke($($argument),*) })
            }
            fn trampoline<R: Returns, Callback: Fn(Self) -> R>(_: &Callback) -> u32 {
                unsafe extern "C" fn invoke<R: Returns, Callback: Fn(($($parameter,)*)) -> R, $($parameter: Scalar),*>(
                    context: *const Callback, $($argument: $parameter),*
                ) -> R::Abi {
                    // The adapter keeps the owning anchor on its Wasm stack
                    // until this call completes. Parameters are ordinary scalars.
                    let callback = unsafe { &*context };
                    callback(($($argument,)*)).into_abi()
                }
                invoke::<R, Callback, $($parameter),*> as *const () as u32
            }
        }
        parameters!(@rest $($parameter $argument),*);
    };
}
parameters!(A a, B b, C c, D d, E e, F f, G g, H h, I i, J j, K k, L l);
