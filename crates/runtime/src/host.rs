use crate::{backend::Backend, Result, Session};
use wasm86_machine::{Environment, HostFunctions};
use wasm86_x86::Compiler;

impl<B: Backend, D: Environment + 'static> Session<B, D> {
    /// Compiles services together, installs their ordinary callable functions,
    /// and publishes address mappings. Replacing a mapping affects fresh visits;
    /// a suspended return retains its original function until completion.
    pub async fn install_host_functions(&mut self, functions: HostFunctions<D>) -> Result<()> {
        if functions.definitions().is_empty() {
            return Ok(());
        }
        let compiled = Compiler::new(self.target.profile)
            .with_cpu_model(self.target.cpu_model)
            .with_execution_budget()
            .compile_host_functions(functions.definitions(), self.next_slot)?;
        let mut module = crate::ExecutableModule::new(compiled.module);
        for export in &compiled.exports {
            module.retain_export(export.clone());
        }
        for entry in compiled.entries {
            module.bind_entry(entry.address, entry.export);
        }
        module.callbacks = functions.into_callbacks();
        self.install_module(module).await
    }
}
