use super::{Dispatch, DispatchEntry};
use crate::{CpuBindings, Hook, Result};
use relocade_compiler::Program;
use relocade_wasm_runtime::{WasmBackend, WasmCompiler};
use relocade_x86::abi;

// Private to this dispatcher. Guest execution exits use different upper tags.
// The address identifies the handler whose stub occupies that dispatch entry.
const HOOK_REQUEST: u64 = 1 << 60;

pub(super) fn request_address(word: u64) -> Option<u32> {
    (word & !u64::from(u32::MAX) == HOOK_REQUEST).then_some(word as u32)
}

impl<B: WasmBackend> Dispatch<B> {
    pub(crate) async fn install_hooks(
        &mut self,
        backend: &mut B,
        bindings: &CpuBindings<B::Memory>,
        hooks: impl IntoIterator<Item = Hook>,
    ) -> Result<()> {
        let hooks: Vec<_> = hooks.into_iter().collect();
        let names: Vec<_> = (0..hooks.len())
            .map(|index| format!("hook_{index}"))
            .collect();
        let mut program = Program::new();
        let mut entries = Vec::with_capacity(hooks.len());
        for (hook, name) in hooks.iter().zip(&names) {
            let function = program.function(abi::RUN.signature(), |body| {
                body.return_(HOOK_REQUEST | u64::from(hook.address))
            })?;
            program.export(name, function)?;
            entries.push(DispatchEntry {
                address: hook.address,
                export: name,
            });
        }
        let module = backend.compiler().compile(&program.compile()?).await?;
        self.install(backend, bindings, &module, &entries).await?;
        // No await or guest entry separates publishing stubs from their handlers.
        for hook in hooks {
            self.hooks.insert(hook.address, hook.handler);
        }
        Ok(())
    }
}
