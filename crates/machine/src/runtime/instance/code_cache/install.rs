//! Compiles and installs modules in one CPU's code cache.

use super::{calls::Calls, hooks::PreparedHandler, installed::ModuleInstance, CodeCache};
use crate::{
    memory::code::ModuleId, runtime::WasmRuntime, worker::HostTurns, CompilationBinding,
    CompiledCode, Cpu, DispatchEntry, FarPointer, Hook, Result,
};
use relocade_wasm_runtime::{Import, Module, WasmBackend, WasmCompiler};
use relocade_x86::abi;

#[derive(Clone, Copy)]
pub(in crate::runtime) enum Installation<'a> {
    Explicit,
    Profiled(&'a [FarPointer]),
}

enum ModuleSource<'a> {
    Wasm(&'a Module),
    Guest(&'a CompiledCode),
}

impl CodeCache {
    pub(in crate::runtime) async fn install(
        &mut self,
        hooks: impl IntoIterator<Item = Hook>,
    ) -> Result<()> {
        let mut hooks = self.hooks.compile(hooks)?;
        let module = self.compile_wasm(&hooks.bytes).await?;
        let handlers = hooks.take_handlers();
        self.install_module(ModuleSource::Wasm(&module), &hooks.entries(), handlers)
    }

    pub(in crate::runtime) async fn install_wasm_bytes(
        &mut self,
        bytes: &[u8],
        entries: &[DispatchEntry<'_>],
    ) -> Result<()> {
        let module = self.compile_wasm(bytes).await?;
        if has_startup_effects(bytes) {
            return self.install_wasm_module(&module, entries);
        }
        let pending = self.with_module_imports(None, |wasm, imports| {
            wasm.instantiate_async(&module, imports)
        });
        let instance = self.host_turns.wait(pending).await?;
        self.register_exports(ModuleInstance::new(instance, None, None), entries)?;
        self.hooks.replace_bindings(entries, Vec::new());
        Ok(())
    }

    pub(in crate::runtime) fn install_wasm_module(
        &mut self,
        module: &Module,
        entries: &[DispatchEntry<'_>],
    ) -> Result<()> {
        self.install_module(ModuleSource::Wasm(module), entries, Vec::new())
    }

    pub(in crate::runtime) fn install_guest_code(
        &mut self,
        code: &CompiledCode,
        installation: Installation<'_>,
    ) -> Result<()> {
        self.retire_pending()?;
        let mut exports = Vec::new();
        let mut fallbacks = Vec::new();
        for binding in code.bindings() {
            match binding {
                CompilationBinding::Export { entry, export } => {
                    let key = entry.packed();
                    if matches!(installation, Installation::Explicit)
                        || !self.installed().dispatch.contains(key)
                    {
                        exports.push(DispatchEntry { key, export });
                    }
                }
                CompilationBinding::Interpreter(entry) => fallbacks.push(entry),
            }
        }
        // A stale module must fail before any of this reply's entries are bound.
        if !exports.is_empty() {
            self.install_module(ModuleSource::Guest(code), &exports, Vec::new())?;
        }
        // A partially decoded profiling batch can also leave a requested root unbound.
        let remaining_roots = match installation {
            Installation::Explicit => &[][..],
            Installation::Profiled(roots) => roots,
        };
        self.bind_interpreter_if_vacant(
            fallbacks.into_iter().chain(remaining_roots.iter().copied()),
        )
    }

    pub(super) fn bind_interpreter_if_vacant(
        &mut self,
        entries: impl IntoIterator<Item = FarPointer>,
    ) -> Result<()> {
        let installed = self.stopped.as_deref_mut().expect("stopped code cache");
        self.guest
            .runtime
            .with_runtime(|wasm| installed.dispatch.bind_interpreter_if_vacant(wasm, entries))
    }

    async fn compile_wasm(&self, bytes: &[u8]) -> Result<Module> {
        let compiler = self.guest.runtime.with_runtime(|wasm| wasm.compiler());
        Ok(self.host_turns.wait(compiler.compile(bytes)).await?)
    }

    fn install_module(
        &mut self,
        source: ModuleSource<'_>,
        entries: &[DispatchEntry<'_>],
        handlers: Vec<PreparedHandler>,
    ) -> Result<()> {
        let guest = self.guest.bindings.guest.clone();
        let checked_source = match source {
            ModuleSource::Wasm(_) => None,
            ModuleSource::Guest(code) => {
                let source = code.source();
                Some((source, source.lock_current_bytes(&guest)?))
            }
        };
        let instance = self.instantiate_module(source)?;
        let host_writes = self.host_writes.clone();
        let (id, installed) = self.register_exports(instance, entries)?;
        if let Some((source, mut registry)) = checked_source {
            installed.track_source(registry.register(id, source), &host_writes);
        }
        // Releasing a host callback can reenter the runtime or write guest memory.
        self.hooks.replace_bindings(entries, handlers);
        Ok(())
    }

    fn instantiate_module(&mut self, source: ModuleSource<'_>) -> Result<ModuleInstance> {
        let (module, guest_code) = match source {
            ModuleSource::Wasm(module) => (module, None),
            ModuleSource::Guest(code) => (code.module(), Some(code)),
        };
        let calls = self
            .guest
            .runtime
            .with_runtime(|wasm| Calls::new(wasm, guest_code, &self.guest.resolve_and_jump))?;
        let instance = self.with_module_imports(calls.as_ref(), |wasm, imports| {
            wasm.instantiate(module, imports)
        })?;
        Ok(ModuleInstance::new(instance, guest_code.cloned(), calls))
    }

    fn register_exports(
        &mut self,
        instance: ModuleInstance,
        entries: &[DispatchEntry<'_>],
    ) -> Result<(ModuleId, &mut ModuleInstance)> {
        let modules = self.stopped.as_deref_mut().expect("stopped code cache");
        self.guest
            .runtime
            .with_runtime(|wasm| modules.register_exports(wasm, instance, entries))
    }

    fn with_module_imports<R>(
        &mut self,
        calls: Option<&Calls>,
        instantiate: impl FnOnce(&mut WasmRuntime, &[Import<'_, WasmRuntime>]) -> R,
    ) -> R {
        let guest = &self.guest;
        let (lookup, functions) = self.installed().dispatch.resources();
        guest.runtime.with_runtime(|wasm| {
            let mut imports = guest.bindings.imports(wasm).to_vec();
            imports.push(wasm.bind(abi::CALL_PROFILE, &self.profiling.memory));
            imports.extend([
                wasm.bind(abi::DISPATCH, &guest.entry_points.dispatch),
                wasm.bind(abi::INTERPRET, &guest.interpret),
                wasm.bind(abi::RESOLVE_CALL, &guest.resolve_call),
            ]);
            imports.extend(guest.io.port_map().imports(wasm));
            imports.extend(guest.io.mmio_map().imports(wasm));
            if let Some(calls) = calls {
                imports.push(calls.import(wasm));
            }
            imports.extend([
                self.write_code.import(),
                wasm.bind(abi::EXECUTION_ENTRIES, &lookup),
                wasm.bind(abi::EXECUTION_FUNCTIONS, &functions),
            ]);
            let _callback = self.write_code.bind(&mut self.stopped);
            instantiate(wasm, &imports)
        })
    }
}

/// A browser instantiation can finish after its Rust future is dropped. Keep startup that
/// can call guest imports or initialize memory or a table within the stopped CPU's borrow.
fn has_startup_effects(bytes: &[u8]) -> bool {
    use wasmparser::{DataKind, ElementKind, Parser, Payload};

    Parser::new(0)
        .parse_all(bytes)
        .any(|payload| match payload {
            Ok(Payload::StartSection { .. } | Payload::UnknownSection { .. }) | Err(_) => true,
            Ok(Payload::DataSection(data)) => data.into_iter().any(|segment| {
                !matches!(segment.map(|segment| segment.kind), Ok(DataKind::Passive))
            }),
            Ok(Payload::ElementSection(elements)) => elements.into_iter().any(|segment| {
                !matches!(
                    segment.map(|segment| segment.kind),
                    Ok(ElementKind::Passive | ElementKind::Declared)
                )
            }),
            _ => false,
        })
}

impl Cpu {
    pub(crate) fn with_host_turns(mut self, host: HostTurns) -> Self {
        self.instance.code.host_turns = host;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::has_startup_effects;
    use wasm_encoder::{
        ConstExpr, DataSection, EntityType, ImportSection, MemoryType, Module, StartSection,
        TypeSection,
    };

    #[test]
    fn instantiation_that_can_run_code_or_write_memory_stays_synchronous() {
        let mut module = Module::new();
        let mut types = TypeSection::new();
        types.ty().function([], []);
        module.section(&types);
        let mut imports = ImportSection::new();
        imports.import("host", "init", EntityType::Function(0));
        imports.import(
            "host",
            "memory",
            MemoryType {
                minimum: 1,
                maximum: None,
                memory64: false,
                shared: false,
                page_size_log2: None,
            },
        );
        module.section(&imports);
        let mut passive = DataSection::new();
        passive.passive([1]);
        let mut active = DataSection::new();
        active.active(0, &ConstExpr::i32_const(0), [1]);

        assert!(!has_startup_effects(
            module.clone().section(&passive).as_slice()
        ));
        assert!(has_startup_effects(
            module.clone().section(&active).as_slice()
        ));
        assert!(has_startup_effects(
            module
                .section(&StartSection { function_index: 0 })
                .as_slice()
        ));
    }
}
