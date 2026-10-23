use crate::{Result, WasmBackend};
use relocade_compiler::abi::Function as Declaration;
use relocade_wasm_bindings::{slots::Tuple, AbiValue, FunctionSignature, Value, WasmRepr};
use std::marker::PhantomData;

/// A function signature whose parameters and results have Rust value codecs.
/// Fields flatten into the same ordered scalar slots on both backends. The
/// supported layouts have up to twelve parameter slots and twelve result slots.
pub trait CallSignature:
    FunctionSignature<
    Parameters: AbiValue<Logical: WasmRepr<Slots: Tuple>>,
    Return: AbiValue<Logical: WasmRepr<Slots: Tuple>>,
>
{
}

impl<S> CallSignature for S where
    S: FunctionSignature<
        Parameters: AbiValue<Logical: WasmRepr<Slots: Tuple>>,
        Return: AbiValue<Logical: WasmRepr<Slots: Tuple>>,
    >
{
}

/// A cached callable export with its declared Rust signature.
/// Use it with the backend that resolved it. The handle retains its instance and
/// imports after the original instance and module handles have been dropped.
pub struct Function<B: ExportFunction<S>, S: CallSignature> {
    pub(crate) handle: B::Handle,
    signature: PhantomData<fn() -> S>,
}

impl<B: ExportFunction<S>, S: CallSignature> Function<B, S> {
    pub(crate) fn new(handle: B::Handle) -> Self {
        Self {
            handle,
            signature: PhantomData,
        }
    }
}

/// Backend support for resolving and calling one Rust function signature.
/// Native calls retain a Wasmtime typed function; browser calls use a generated
/// Wasm adapter in Rust's function table. Both use the shared logical codecs.
pub trait ExportFunction<S: CallSignature>: WasmBackend {
    type Handle;

    fn resolve_function(
        &mut self,
        instance: &Self::Instance,
        declaration: Declaration<S>,
    ) -> Result<Self::Handle>;

    fn invoke_function(
        &mut self,
        function: &Self::Handle,
        arguments: Value<S::Parameters>,
    ) -> Value<S::Return>;

    fn function_import(&self, function: &Self::Handle) -> Self::ImportFunction;
}
