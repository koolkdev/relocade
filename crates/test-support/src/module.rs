use std::sync::OnceLock;

/// Immutable Wasm bytes with lazy Wasmtime compilation.
/// Execution hosts still create fresh stores, imports and instances per case.
pub struct Module {
    bytes: Vec<u8>,
    compiled: OnceLock<wasmtime::Module>,
}

impl Module {
    pub fn new(bytes: &[u8]) -> Self {
        Self {
            bytes: bytes.into(),
            compiled: OnceLock::new(),
        }
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn wasmtime(&self) -> &wasmtime::Module {
        self.compiled.get_or_init(|| {
            wasmtime::Module::new(crate::engine(), &self.bytes).expect("compile test module")
        })
    }
}
