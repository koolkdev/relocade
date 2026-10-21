use crate::Result;
use relocade_wasm_runtime::{WasmBackend, WasmCompiler};
use relocade_x86::{Compiler, ExecutionTarget};

pub(super) async fn dispatcher<B: WasmBackend>(
    backend: &B,
    target: ExecutionTarget,
) -> Result<B::Module> {
    let compiler = Compiler::new(target.profile).with_cpu_model(target.cpu_model);
    let dispatcher = compiler.compile_dispatcher()?;
    Ok(backend.compiler().compile(&dispatcher.bytes).await?)
}

pub(super) async fn interpreter<B: WasmBackend>(
    backend: &B,
    target: ExecutionTarget,
) -> Result<B::Module> {
    let bytes = relocade_interpreter::module(target.profile);
    let compiler = backend.compiler();
    Ok(if let Some(cache) = backend.module_cache() {
        cache
            .get_or_compile(bytes, |bytes| compiler.compile(bytes))
            .await?
    } else {
        compiler.compile(bytes).await?
    })
}
