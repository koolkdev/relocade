//! CPU-local module instances, callable exports and decoded guest-code coverage.

mod calls;
pub(super) use calls::Calls;
mod table;

use super::hooks::Hooks;
use crate::runtime::{
    memory::{HostWritePage, HostWritePages},
    WasmRuntime, WasmRuntimeHandle,
};
use crate::{CompiledCode, DispatchEntry, FarPointer, InstalledCode, Result};
use relocade_wasm_runtime::{ExportFunction, Import};
use relocade_x86::abi;
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
    rc::Rc,
    sync::Arc,
};
use table::DispatchTable;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ModuleId(usize);

pub(super) struct ModuleInstance<B: ExportFunction<abi::ExecutionSignature>> {
    instance: B::Instance,
    entry_keys: Vec<u32>,
    code: Option<CompiledCode>,
    registered_source: Option<Arc<InstalledCode>>,
    host_write_pages: Vec<Rc<HostWritePage>>,
    calls: Option<Calls<B>>,
    linear_pages: BTreeSet<u32>,
}

impl<B: ExportFunction<abi::ExecutionSignature>> ModuleInstance<B> {
    pub(super) fn new(
        instance: B::Instance,
        code: Option<CompiledCode>,
        calls: Option<Calls<B>>,
    ) -> Self {
        let linear_pages = code
            .iter()
            .flat_map(|code| code.ranges())
            .flat_map(|range| (range.start >> 12)..=((range.end - 1) >> 12))
            .map(|page| page as u32)
            .collect();
        Self {
            instance,
            entry_keys: Vec::new(),
            code,
            registered_source: None,
            host_write_pages: Vec::new(),
            calls,
            linear_pages,
        }
    }

    pub(super) fn code(&self) -> Option<&CompiledCode> {
        self.code.as_ref()
    }

    pub(super) fn track_source(
        &mut self,
        source: Arc<InstalledCode>,
        host_writes: &Rc<HostWritePages>,
    ) {
        self.host_write_pages = source
            .physical_pages()
            .map(|page| host_writes.hold(page))
            .collect();
        self.registered_source = Some(source);
    }
}

impl<B: ExportFunction<abi::ExecutionSignature>> Drop for ModuleInstance<B> {
    fn drop(&mut self) {
        if let Some(code) = &self.registered_source {
            code.unregister();
        }
    }
}

pub(super) struct Modules<B: ExportFunction<abi::ExecutionSignature>> {
    table: DispatchTable<B>,
    modules: BTreeMap<ModuleId, ModuleInstance<B>>,
    next_module_id: usize,
    by_linear_page: BTreeMap<u32, BTreeSet<ModuleId>>,
    pub(super) hooks: Hooks,
}

impl<B: ExportFunction<abi::ExecutionSignature>> Modules<B> {
    pub(super) fn new(backend: &mut B) -> Result<Self> {
        Ok(Self {
            table: DispatchTable::new(backend)?,
            modules: BTreeMap::new(),
            next_module_id: 0,
            by_linear_page: BTreeMap::new(),
            hooks: Hooks::default(),
        })
    }

    pub(super) fn imports<'a>(&'a self, backend: &B) -> [Import<'a, B>; 2]
    where
        B: 'a,
    {
        self.table.imports(backend)
    }

    pub(super) fn has_entry(&self, key: u32) -> bool {
        self.table.contains(key)
    }

    pub(super) fn install_interpreter(
        &mut self,
        backend: &mut B,
        instance: &B::Instance,
    ) -> Result<()> {
        self.table.install_interpreter(backend, instance)
    }

    pub(super) fn install_interpreter_fallback(
        &mut self,
        backend: &mut B,
        entry: FarPointer,
    ) -> Result<()> {
        self.table.install_interpreter_fallback(backend, entry)
    }

    pub(super) fn register_exports(
        &mut self,
        backend: &mut B,
        mut module: ModuleInstance<B>,
        entries: &[DispatchEntry<'_>],
    ) -> Result<&mut ModuleInstance<B>> {
        let id = ModuleId(self.next_module_id);
        module.entry_keys = self
            .table
            .register_exports(backend, id, &module.instance, entries)?;
        if let Some(calls) = &module.calls {
            self.table.register_calls(backend, id, calls)?;
        }
        for &page in &module.linear_pages {
            self.by_linear_page.entry(page).or_default().insert(id);
        }
        self.next_module_id = self
            .next_module_id
            .checked_add(1)
            .expect("module identity fits usize");
        Ok(self.modules.entry(id).insert_entry(module).into_mut())
    }
}

impl Modules<WasmRuntime> {
    /// Retire each complete linked version that covers a changed byte.
    pub(super) fn invalidate(
        &mut self,
        runtime: &WasmRuntimeHandle,
        range: Range<u64>,
    ) -> Result<()> {
        if range.is_empty() {
            return Ok(());
        }
        let mut retired_modules: BTreeSet<_> = self
            .by_linear_page
            .range((range.start >> 12) as u32..=((range.end - 1) >> 12) as u32)
            .flat_map(|(_, modules)| modules.iter().copied())
            .collect();
        retired_modules.retain(|id| {
            let ranges = self.modules[id]
                .code()
                .expect("indexed compilation")
                .ranges();
            let index = ranges.partition_point(|source| source.end <= range.start);
            ranges
                .get(index)
                .is_some_and(|source| source.start < range.end)
        });
        self.retire(runtime, retired_modules)
    }

    fn retire(
        &mut self,
        runtime: &WasmRuntimeHandle,
        retired_modules: BTreeSet<ModuleId>,
    ) -> Result<()> {
        for id in &retired_modules {
            if let Some(calls) = &self.modules[id].calls {
                self.table.unregister_calls(*id, calls);
            }
        }
        let exports = retired_modules.iter().flat_map(|&id| {
            self.modules[&id]
                .entry_keys
                .iter()
                .map(move |&key| (id, key))
        });
        // Release the runtime borrow before dropping the modules' host-write pages.
        runtime.with_runtime(|wasm| self.table.remove(wasm, exports))?;
        for id in retired_modules {
            let module = self.modules.remove(&id).expect("installed module");
            for &page in &module.linear_pages {
                let modules = self
                    .by_linear_page
                    .get_mut(&page)
                    .expect("registered source page");
                modules.remove(&id);
                if modules.is_empty() {
                    self.by_linear_page.remove(&page);
                }
            }
        }
        Ok(())
    }

    pub(super) fn covering(
        &self,
        address: u32,
    ) -> impl Iterator<Item = &ModuleInstance<WasmRuntime>> {
        self.by_linear_page
            .get(&(address >> 12))
            .into_iter()
            .flatten()
            .map(|id| &self.modules[id])
            .filter(move |module| {
                module.code().is_some_and(|code| {
                    let address = u64::from(address);
                    let ranges = code.ranges();
                    let index = ranges.partition_point(|range| range.end <= address);
                    ranges
                        .get(index)
                        .is_some_and(|range| range.start <= address)
                })
            })
    }
}
