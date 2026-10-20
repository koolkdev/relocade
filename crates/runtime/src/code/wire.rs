//! Private-build messages for a compiler in a separate Wasm instance.
//! Ownership stays on the execution side; only copied inputs and metadata cross.

use super::{capture::CodeSnapshot, compile::CompilationInput, PreparedGroup};
use crate::{Error, Result};

impl CodeSnapshot {
    /// Encodes a job for a trusted worker running the same relocade build.
    /// This format is not a persistent cache or an untrusted-input protocol.
    pub fn encode(&self) -> Result<Vec<u8>> {
        postcard::to_allocvec(&self.input)
            .map_err(|error| Error::wasm("encode compiler job", error))
    }

    /// Worker entry point: discovers and lowers a job without any runtime.
    pub fn compile_message(bytes: &[u8]) -> Result<Option<PreparedGroup>> {
        let input: CompilationInput = postcard::from_bytes(bytes)
            .map_err(|error| Error::wasm("decode compiler job", error))?;
        Self {
            owner: Default::default(),
            input,
        }
        .compile()
    }

    /// Reunites this capture's ownership with its worker's result. Backends must
    /// route the response to the originating job, and compile its Wasm unchanged.
    pub fn complete_message(self, bytes: &[u8]) -> Result<PreparedGroup> {
        let code = postcard::from_bytes(bytes)
            .map_err(|error| Error::wasm("decode compiler result", error))?;
        Ok(PreparedGroup {
            owner: self.owner,
            code,
        })
    }
}

impl PreparedGroup {
    /// Encodes publication metadata after the worker has compiled `self.wasm()`.
    /// Engine code travels separately; returning the original Wasm is unnecessary.
    pub fn into_message(mut self) -> Result<Vec<u8>> {
        self.code.module.bytes.clear();
        postcard::to_allocvec(&self.code)
            .map_err(|error| Error::wasm("encode compiler result", error))
    }
}
