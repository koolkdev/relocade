use crate::Result;
use relocade_wasm_runtime::{ModuleCache, WasmCompiler};
use relocade_x86::{Compiler, CompilerConfig};

pub(super) async fn dispatcher<C: WasmCompiler>(
    compiler: &C,
    config: CompilerConfig,
) -> Result<C::Module> {
    let dispatcher = Compiler::from_config(config).compile_dispatcher()?;
    Ok(compiler.compile(&dispatcher.bytes).await?)
}

pub(super) async fn interpreter<C: WasmCompiler>(
    compiler: &C,
    cache: Option<&ModuleCache<C::Module>>,
    config: CompilerConfig,
) -> Result<C::Module> {
    let bytes = relocade_interpreter::module_with_memory(config.entry, config.memory_backend)
        .ok_or(crate::ConfigurationError::UnsupportedMemoryBackend)?;
    Ok(if let Some(cache) = cache {
        cache
            .get_or_compile(bytes, |bytes| compiler.compile(bytes))
            .await?
    } else {
        compiler.compile(bytes).await?
    })
}
