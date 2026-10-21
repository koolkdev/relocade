//! Guest-code compilation on a typed worker endpoint.
//!
//! The embedding creates the worker and its backend. This module owns the same
//! lowering and compilation handler on native and browser workers; startup,
//! transport and the request loop belong to `relocade-worker`.

use crate::{wasm::WasmBackend, Result};
use relocade_worker::{Message, RequestEndpoint, RequestWorker};

pub type CompilerWorker<M> = RequestWorker<Vec<u8>, Option<Compilation<M>>>;

/// Lowering output. Metadata belongs to the consumer's request format.
pub struct LoweredModule {
    pub wasm: Vec<u8>,
    pub metadata: Vec<u8>,
}

/// Compiled code ready to instantiate with an execution owner's private imports.
#[derive(Message)]
pub struct Compilation<M> {
    pub module: M,
    pub metadata: Vec<u8>,
}

/// Serves owned compiler requests using worker-local state. None means lowering
/// does not support the input. Compilation errors fail only their request.
/// Guest instances and imports remain with the execution owner.
pub async fn run<B: WasmBackend>(
    endpoint: RequestEndpoint<Vec<u8>, Option<Compilation<B::Module>>>,
    backend: B,
    mut lower: impl FnMut(&[u8]) -> Result<Option<LoweredModule>>,
) -> relocade_worker::Result<()>
where
    B::Module: Message,
{
    endpoint
        .serve(async |request: Vec<u8>| {
            let Some(lowered) = lower(&request).map_err(worker_error)? else {
                return Ok(None);
            };
            let module = backend.compile(&lowered.wasm).await.map_err(worker_error)?;
            Ok(Some(Compilation {
                module,
                metadata: lowered.metadata,
            }))
        })
        .await
}

fn worker_error(error: crate::Error) -> relocade_worker::Error {
    relocade_worker::Error::Failed(error.to_string())
}
