use crate::Result;
use relocade_wasm_runtime::{ModuleCache, WasmCompiler};
use relocade_x86::{Compiler, ExecutionTarget};

pub(super) async fn dispatcher<C: WasmCompiler>(
    compiler: &C,
    target: ExecutionTarget,
) -> Result<C::Module> {
    let dispatcher = Compiler::from_target(target).compile_dispatcher()?;
    Ok(compiler.compile(&dispatcher.bytes).await?)
}

pub(super) async fn interpreter<C: WasmCompiler>(
    compiler: &C,
    cache: Option<&ModuleCache<C::Module>>,
    target: ExecutionTarget,
) -> Result<C::Module> {
    let bytes = relocade_interpreter::module_with_memory(target.profile, target.memory_backend)
        .ok_or(crate::ConfigurationError::UnsupportedMemoryBackend)?;
    Ok(if let Some(cache) = cache {
        cache
            .get_or_compile(bytes, |bytes| compiler.compile(bytes))
            .await?
    } else {
        compiler.compile(bytes).await?
    })
}
