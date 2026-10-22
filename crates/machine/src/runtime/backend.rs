use relocade_compiler::{I16, I32, I64};
use relocade_wasm_runtime::{HostFunction, WasmBackend};
use relocade_x86::abi::{QuerySegmentDescriptorSignature, ResolveSegmentSignature};

/// A Wasm backend that supports descriptor, port, MMIO and host-call imports.
///
/// Implemented automatically by backends with the required typed host bindings.
/// Embeddings can use this bound without spelling out the runtime's Wasm ABI.
pub trait RuntimeBackend:
    WasmBackend
    + HostFunction<fn(I16, I32) -> I32>
    + HostFunction<fn(I16, I32, I32)>
    + HostFunction<fn(I32, I32) -> I64>
    + HostFunction<fn(I32, I32, I64)>
    + HostFunction<fn() -> I64>
    + HostFunction<ResolveSegmentSignature>
    + HostFunction<QuerySegmentDescriptorSignature>
    + HostFunction<fn(I16)>
{
}

impl<B> RuntimeBackend for B where
    B: WasmBackend
        + HostFunction<fn(I16, I32) -> I32>
        + HostFunction<fn(I16, I32, I32)>
        + HostFunction<fn(I32, I32) -> I64>
        + HostFunction<fn(I32, I32, I64)>
        + HostFunction<fn() -> I64>
        + HostFunction<ResolveSegmentSignature>
        + HostFunction<QuerySegmentDescriptorSignature>
        + HostFunction<fn(I16)>
{
}
