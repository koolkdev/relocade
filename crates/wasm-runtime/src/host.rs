use crate::{CallSignature, Import, WasmBackend};
use relocade_compiler::abi::{Function as Declaration, FunctionSignature};
use relocade_wasm_bindings::Value;
use std::marker::PhantomData;

/// Creates host functions using this context's runtime capabilities.
/// The callback receives a fresh scope on each invocation. Its lifetime is
/// independent of the scope used to create the function.
pub trait FunctionContext<S: CallSignature> {
    type Backend: WasmBackend;
    type CallbackContext<'a>;

    #[doc(hidden)]
    fn wrap_function(
        &mut self,
        callback: impl Fn(Self::CallbackContext<'_>, Value<S::Parameters>) -> Value<S::Return> + 'static,
    ) -> <Self::Backend as WasmBackend>::ImportFunction;
}

/// A reusable engine function with its logical signature intact.
/// Produced from a host callback or a resolved export. Clones share the same
/// function; tables retain it after these handles are dropped. Use it only with
/// the backend that created it.
pub struct FunctionRef<B: WasmBackend, S: FunctionSignature> {
    pub(crate) handle: B::ImportFunction,
    signature: PhantomData<fn() -> S>,
}

impl<B: WasmBackend, S: FunctionSignature> Clone for FunctionRef<B, S> {
    fn clone(&self) -> Self {
        Self::from_handle(self.handle.clone())
    }
}

impl<B: WasmBackend, S: FunctionSignature> FunctionRef<B, S> {
    /// Binds this prepared function to a named import of the same signature.
    pub fn import<'a>(&self, declaration: Declaration<'a, S>) -> Import<'a, B> {
        Import::function(declaration, self.handle.clone())
    }

    pub(crate) fn from_handle(handle: B::ImportFunction) -> Self {
        Self {
            handle,
            signature: PhantomData,
        }
    }
}

impl<B: WasmBackend, S: CallSignature> FunctionRef<B, S> {
    /// Creates a concrete callback with the supplied context's capabilities.
    /// The first argument is scoped to the invocation; the remaining arguments
    /// form the signature's Rust tuple, as in [`crate::Function::call`].
    /// Creating the function does not enter guest code.
    ///
    /// Use [`WasmBackend::context`] for host state, memory and table access.
    ///
    /// Captures stay on the execution thread and need not implement Send or Sync.
    /// They must not retain the backend itself. Native stores retain functions
    /// until teardown; prepare reusable functions once and install their handles
    /// at state changes.
    ///
    /// ```
    /// use relocade_compiler::abi::Function;
    /// use relocade_wasm_runtime::{Backend, FunctionRef, WasmBackend};
    /// const READ: Function<fn(u16, u32) -> u32> = Function::new("example", "read");
    /// let mut backend = Backend::new().unwrap();
    /// let read = FunctionRef::new(&mut backend.context(), |_, (port, width)| {
    ///     u32::from(port) + width
    /// });
    /// let import = read.import(READ);
    /// ```
    ///
    /// Callback results must match the declared logical type:
    /// ```compile_fail
    /// use relocade_wasm_runtime::{Backend, FunctionRef, WasmBackend};
    /// let mut backend = Backend::new().unwrap();
    /// FunctionRef::<_, fn(u16) -> u32>::new(&mut backend.context(), |_, (_,)| 0_i64);
    /// ```
    /// Parameter types must match too:
    /// ```compile_fail
    /// use relocade_wasm_runtime::{Backend, FunctionRef, WasmBackend};
    /// let mut backend = Backend::new().unwrap();
    /// FunctionRef::<_, fn(u16)>::new(&mut backend.context(), |_, (_value,): (u32,)| {});
    /// ```
    /// Callback scopes cannot escape their invocation:
    /// ```compile_fail
    /// use relocade_wasm_runtime::{Backend, Context, FunctionRef, WasmBackend};
    /// use std::{cell::RefCell, rc::Rc};
    /// let saved: Rc<RefCell<Option<Context<'static, ()>>>> = Rc::default();
    /// let mut backend = Backend::new().unwrap();
    /// FunctionRef::<_, fn()>::new(&mut backend.context(), move |context, ()| {
    ///     *saved.borrow_mut() = Some(context);
    /// });
    /// ```
    pub fn new<C: FunctionContext<S, Backend = B>>(
        context: &mut C,
        callback: impl Fn(C::CallbackContext<'_>, Value<S::Parameters>) -> Value<S::Return> + 'static,
    ) -> Self {
        Self::from_handle(context.wrap_function(callback))
    }
}
