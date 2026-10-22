use crate::RuntimeBackend;
use relocade_compiler::{I16, I32, I64};
use relocade_wasm_runtime::{HostSignature, Import};
use relocade_x86::abi;

/// Synchronous device callbacks invoked inside guest execution.
///
/// CPU registers need not be published at these boundaries. Callbacks can access
/// device handles in backend state and shared guest RAM for DMA. An embedding
/// may retain the same device handles in ordinary Rust hooks.
pub struct IoBindings<B: RuntimeBackend> {
    pub read_port: <fn(I16, I32) -> I32 as HostSignature<B>>::Callback,
    pub write_port: <fn(I16, I32, I32) as HostSignature<B>>::Callback,
    pub read_mmio: <fn(I32, I32) -> I64 as HostSignature<B>>::Callback,
    pub write_mmio: <fn(I32, I32, I64) as HostSignature<B>>::Callback,
}

impl<B: RuntimeBackend> Default for IoBindings<B> {
    /// Unconnected reads return all ones; writes are ignored.
    fn default() -> Self {
        Self {
            read_port: Box::new(|_, _, _| -1),
            write_port: Box::new(|_, _, _, _| {}),
            read_mmio: Box::new(|_, _, _| -1),
            write_mmio: Box::new(|_, _, _, _| {}),
        }
    }
}

impl<B: RuntimeBackend> IoBindings<B> {
    pub(super) fn bind(self, backend: &mut B) -> Vec<Import<'static, B>> {
        vec![
            backend.bind_host(abi::READ_PORT, self.read_port),
            backend.bind_host(abi::WRITE_PORT, self.write_port),
            backend.bind_host(abi::READ_MMIO, self.read_mmio),
            backend.bind_host(abi::WRITE_MMIO, self.write_mmio),
        ]
    }
}
