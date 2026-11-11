use crate::Result;
use relocade_wasm_runtime::{ModuleCache, WasmCompiler};
use relocade_x86::{Compiler, ExecutionTarget};

pub(super) async fn dispatcher<C: WasmCompiler>(
    compiler: &C,
    target: ExecutionTarget,
) -> Result<C::Module> {
    let dispatcher = Compiler::new(target.profile)
        .with_cpu_model(target.cpu_model)
        .with_instruction_budgets(target.instruction_budgets)
        .compile_dispatcher()?;
    Ok(compiler.compile(&dispatcher.bytes).await?)
}

pub(super) async fn interpreter<C: WasmCompiler>(
    compiler: &C,
    cache: Option<&ModuleCache<C::Module>>,
    target: ExecutionTarget,
) -> Result<C::Module> {
    let bytes = relocade_interpreter::module(target.profile);
    Ok(if let Some(cache) = cache {
        cache
            .get_or_compile(bytes, |bytes| compiler.compile(bytes))
            .await?
    } else {
        compiler.compile(bytes).await?
    })
}
