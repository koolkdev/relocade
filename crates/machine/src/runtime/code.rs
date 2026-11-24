use crate::Result;
use relocade_wasm_runtime::{ModuleCache, WasmCompiler};
use relocade_x86::{Compiler, CompilerConfig};

pub(super) async fn dispatcher<C: WasmCompiler>(
    compiler: &C,
    config: CompilerConfig,
) -> Result<C::Module> {
    let dispatcher = Compiler::new(config).compile_dispatcher()?;
    Ok(compiler.compile(&dispatcher.bytes).await?)
}

pub(super) async fn pc_interpreter<C: WasmCompiler>(
    compiler: &C,
    cache: Option<&ModuleCache<C::Module>>,
) -> Result<C::Module> {
    let bytes = relocade_interpreter::pc();
    Ok(if let Some(cache) = cache {
        cache
            .get_or_compile(bytes, |bytes| compiler.compile(bytes))
            .await?
    } else {
        compiler.compile(bytes).await?
    })
}
