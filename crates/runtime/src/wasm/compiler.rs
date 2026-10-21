use crate::{
    worker::{Message, Thread},
    Error, Result,
};
use std::{future::Future, sync::OnceLock};

/// Engine-only compilation state that can be cloned onto a worker.
/// Compiled modules may return to an execution owner, while the compilation
/// future itself stays on the worker and need not be Send.
pub trait WasmCompiler: Clone + Send + 'static {
    type Module: 'static;

    fn compile(&self, bytes: &[u8]) -> impl Future<Output = Result<Self::Module>>;
}

/// A persistent worker for lowering and engine compilation. Backends share this
/// owner alongside their engine and module cache; it contains no live guest state.
pub struct CompilerWorker<C> {
    thread: OnceLock<Thread>,
    compiler: C,
}

impl<C: WasmCompiler> CompilerWorker<C>
where
    Message<std::result::Result<C::Module, String>>: web_thread::Post,
{
    /// Starts the thread on the first request and reuses it for later requests.
    pub fn new(compiler: C) -> Self {
        Self {
            thread: OnceLock::new(),
            compiler,
        }
    }

    /// Owns the lowering input so dropping the waiting future cannot leave the
    /// worker borrowing caller state. Diagnostics cross the transport as strings.
    pub async fn compile(
        &self,
        lower: impl FnOnce() -> Result<Vec<u8>> + Send + 'static,
    ) -> Result<C::Module> {
        let compiler = self.compiler.clone();
        let Message(result) = self
            .thread
            .get_or_init(Thread::new)
            .run((), move |()| async move {
                let result = async {
                    let bytes = lower()?;
                    compiler.compile(&bytes).await
                }
                .await;
                Message(result.map_err(|error| error.to_string()))
            })
            .await
            .map_err(|error| Error::wasm("compiler worker", error))?;
        result.map_err(|error| Error::wasm("compile module", error))
    }
}
