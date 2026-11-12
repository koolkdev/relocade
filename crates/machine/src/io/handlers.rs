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
    pub(super) function: OnceCell<(Weak<State>, FunctionRef<WasmRuntime, S>)>,
}

impl<S: FunctionSignature, C: ?Sized> Handler<S, C> {
    pub(super) fn function(&self, context: &mut IoContext<'_>) -> &FunctionRef<WasmRuntime, S> {
        self.check_owner(context);
        let (_, function) = self
            .function
            .get_or_init(|| (context.bus.clone(), (self.bind)(context)));
        function
    }

    pub(super) fn check_owner(&self, context: &IoContext<'_>) {
        if let Some((owner, _)) = self.function.get() {
            assert!(
                Weak::ptr_eq(owner, context.bus),
                "I/O operation is bound to another bus"
            );
        }
    }
}
