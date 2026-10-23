use relocade_compiler::{I16, I32, I64};
use relocade_wasm_runtime::{ExportFunction, HostFunction, WasmBackend};

/// A Wasm backend that supports dispatch and the runtime's port, MMIO and hook imports.
///
/// Implemented automatically by backends with the required typed function bindings.
/// Embeddings can use this bound without spelling out the runtime's Wasm ABI.
pub trait RuntimeBackend:
    WasmBackend
    + ExportFunction<fn() -> I64>
    + HostFunction<fn(I16, I32) -> I32>
    + HostFunction<fn(I16, I32, I32)>
    + HostFunction<fn(I32, I32) -> I64>
    + HostFunction<fn(I32, I32, I64)>
    + HostFunction<fn() -> I64>
{
}

impl<B> RuntimeBackend for B where
    B: WasmBackend
        + ExportFunction<fn() -> I64>
        + HostFunction<fn(I16, I32) -> I32>
        + HostFunction<fn(I16, I32, I32)>
        + HostFunction<fn(I32, I32) -> I64>
        + HostFunction<fn(I32, I32, I64)>
        + HostFunction<fn() -> I64>
{
}
