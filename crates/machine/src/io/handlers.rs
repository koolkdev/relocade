//! One prepared host operation and its lazily created Wasm function.

use super::{IoContext, State};
use relocade_compiler::abi::FunctionSignature;
use relocade_wasm_runtime::{FunctionRef, WasmRuntime};
use std::{
    cell::OnceCell,
    rc::{Rc, Weak},
};

type Bind<S> = dyn Fn(&mut IoContext<'_>) -> FunctionRef<WasmRuntime, S>;

pub(super) struct Handler<S: FunctionSignature, C: ?Sized> {
    pub(super) callback: Rc<C>,
    pub(super) bind: Box<Bind<S>>,
    pub(super) owner: OnceCell<Weak<State>>,
    pub(super) function: OnceCell<FunctionRef<WasmRuntime, S>>,
}

impl<S: FunctionSignature, C: ?Sized> Handler<S, C> {
    pub(super) fn bind_to(&self, bus: &Weak<State>) {
        let owner = self.owner.get_or_init(|| bus.clone());
        assert!(
            Weak::ptr_eq(owner, bus),
            "I/O operation is bound to another bus"
        );
    }

    pub(super) fn function(&self, context: &mut IoContext<'_>) -> &FunctionRef<WasmRuntime, S> {
        self.function.get_or_init(|| (self.bind)(context))
    }
}
