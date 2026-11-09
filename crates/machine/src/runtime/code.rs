use crate::Result;
use relocade_wasm_runtime::{WasmBackend, WasmCompiler};
use relocade_x86::{Compiler, ExecutionTarget};

pub(super) async fn dispatcher<B: WasmBackend>(
    backend: &B,
    target: ExecutionTarget,
) -> Result<B::Module> {
    let compiler = Compiler::new(target.profile)
        .with_cpu_model(target.cpu_model)
        .with_instruction_budgets(target.instruction_budgets);
    let dispatcher = compiler.compile_dispatcher()?;
    Ok(backend.compiler().compile(&dispatcher.bytes).await?)
}

pub(super) async fn interpreter<B: WasmBackend>(
    backend: &B,
    target: ExecutionTarget,
) -> Result<B::Module> {
    let compiler = backend.compiler();
    let compile = || async {
        let bytes = relocade_interpreter::load_or_generate(target.profile, || async {
            relocade_x86::build_interpreter(target.profile)?.compile()
        })
        .await?;
        Ok(compiler.compile(&bytes).await?)
    };
    match backend.module_cache() {
        Some(cache) => {
            cache
                .get_or_compile(
                    relocade_interpreter::key(target.profile).as_bytes(),
                    compile,
                )
                .await
        }
        None => compile().await,
    }
}
