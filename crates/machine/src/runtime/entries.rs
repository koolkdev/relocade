//! Publishes service pairs and guest modules without stranding retained invocations.

use super::{Cpu, DispatchEntry, Hook, Result};
use relocade_compiler::Program;
use relocade_wasm_runtime::{WasmBackend, WasmCompiler};
use relocade_x86::{abi, ExecutionExitValues, HostExitValues};
use std::collections::HashSet;

impl<T: 'static> Cpu<T> {
    /// Links a module to this runtime and publishes its callable exports.
    /// Each export has signature `() -> ExecutionExit` and owns its entry checks, CPU
    /// publication and continuation. Addresses are linear CS.base + EIP, before
    /// paging/A20. Installing an existing address replaces its function.
    ///
    /// The caller must maintain the module's execution assumptions and invalidate
    /// or replace captured code when guest bytes or mappings change. Replacing
    /// either label of a host service removes its entire pair. Parked invocations
    /// must complete or be abandoned before replacing their entries.
    /// Module start functions must not enter guest dispatch or host hooks;
    /// execution belongs to `run`.
    ///
    /// # Panics
    /// Panics while a host call is pending or when replacing a parked
    /// invocation's entry/resume pair. Other returned guest execution
    /// exits permit access even when interrupted calls remain suspended.
    #[allow(
        clippy::await_holding_refcell_ref,
        reason = "Installation excludes guest execution and pending host calls."
    )]
    pub async fn install_module(
        &mut self,
        bytes: &[u8],
        entries: &[DispatchEntry<'_>],
    ) -> Result<()> {
        let suspended: Vec<_> = self
            .suspended_calls
            .iter()
            .map(|frame| frame.address)
            .collect();
        let mut borrow = self.machine_mut();
        let machine = &mut *borrow;
        let replaced: Vec<_> = machine
            .hooks
            .iter()
            .filter_map(|(&address, service)| {
                entries
                    .iter()
                    .any(|entry| entry.address == address || entry.address == service.resume)
                    .then_some((address, service.resume))
            })
            .collect();
        assert!(
            replaced
                .iter()
                .all(|(address, _)| !suspended.contains(address)),
            "parked invocations require their service's entry/resume pair"
        );
        let removed: Vec<_> = replaced
            .iter()
            .flat_map(|&(entry, resume)| [entry, resume])
            .collect();
        let compiler = machine.backend.compiler();
        let module = compiler.compile(bytes).await?;
        machine
            .dispatch
            .install(
                &mut machine.backend,
                &machine.bindings,
                &module,
                entries,
                &removed,
            )
            .await?;
        for (address, _) in replaced {
            machine.hooks.remove(&address);
        }
        Ok(())
    }

    /// Installs each service's entry/resume pair through ordinary guest dispatch.
    /// The environment owns their backing bytes and completion sequence. Entries
    /// consume no guest instructions and preserve the current instruction target.
    /// Each invocation owns its future; CPU operations borrow only for storage access.
    ///
    /// Replacing a service updates both labels together. Parked invocations may
    /// keep using a replacement with the same pair; changing their resume label
    /// requires completing or abandoning them first.
    ///
    /// # Panics
    /// Panics while a host call is pending, on overlapping pairs, or when changing
    /// a parked service's resume address.
    #[allow(
        clippy::await_holding_refcell_ref,
        reason = "Installation excludes guest execution and pending host calls."
    )]
    pub async fn install_hooks(&mut self, hooks: impl IntoIterator<Item = Hook<T>>) -> Result<()> {
        let suspended: Vec<_> = self
            .suspended_calls
            .iter()
            .map(|frame| frame.address)
            .collect();
        let mut borrow = self.machine_mut();
        let machine = &mut *borrow;
        let hooks: Vec<_> = hooks.into_iter().collect();
        let replacing: HashSet<_> = hooks.iter().map(|hook| hook.address).collect();
        let mut addresses = HashSet::new();
        for (&address, service) in &machine.hooks {
            if !replacing.contains(&address) {
                addresses.extend([address, service.resume]);
            }
        }
        let mut removed = Vec::new();
        for hook in &hooks {
            assert!(
                addresses.insert(hook.address) && addresses.insert(hook.service.resume),
                "service entry/resume pairs must not overlap"
            );
            if let Some(old) = machine.hooks.get(&hook.address) {
                if old.resume != hook.service.resume {
                    assert!(
                        !suspended.contains(&hook.address),
                        "parked invocations require their service's resume label"
                    );
                    removed.extend([hook.address, old.resume]);
                }
            }
        }
        let names: Vec<_> = (0..hooks.len())
            .map(|index| [format!("hook_{index}"), format!("resume_{index}")])
            .collect();
        let mut program = Program::new();
        let mut handlers = Vec::new();
        let mut entries = Vec::new();
        for (hook, names) in hooks.into_iter().zip(&names) {
            let address = hook.address;
            for (entry, name, exit) in [
                (address, &names[0], HostExitValues::call(address)),
                (
                    hook.service.resume,
                    &names[1],
                    HostExitValues::resume(address),
                ),
            ] {
                let function = program.function::<abi::ExecutionSignature>(move |body, ()| {
                    body.return_(ExecutionExitValues::host(exit))
                });
                program.export(name, function)?;
                entries.push(DispatchEntry {
                    address: entry,
                    export: name,
                });
            }
            handlers.push((address, hook.service));
        }
        let bytes = program.compile()?;
        let compiler = machine.backend.compiler();
        let module = compiler.compile(&bytes).await?;
        machine
            .dispatch
            .install(
                &mut machine.backend,
                &machine.bindings,
                &module,
                &entries,
                &removed,
            )
            .await?;
        // Publish handlers only after both callable entries exist.
        machine.hooks.extend(handlers);
        Ok(())
    }
}
