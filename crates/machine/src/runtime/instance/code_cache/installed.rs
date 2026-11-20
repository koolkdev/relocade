//! CPU-local module instances, callable exports and decoded guest-code coverage.

use super::{calls::Calls, dispatch::DispatchTable};
use crate::memory::code::ModuleId;
use crate::runtime::{
    memory::{HostWritePage, HostWritePages},
    WasmRuntime,
};
use crate::{CompiledCode, DispatchEntry, InstalledCode, Result};
use relocade_wasm_runtime::{Context, WasmBackend};
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
    rc::Rc,
    sync::Arc,
};

pub(super) struct ModuleInstance {
    instance: <WasmRuntime as WasmBackend>::Instance,
    entry_keys: Vec<u32>,
    code: Option<CompiledCode>,
    registered_source: Option<Arc<InstalledCode>>,
    host_write_pages: Vec<Rc<HostWritePage>>,
    pub(super) calls: Option<Calls>,
    linear_pages: BTreeSet<u32>,
}

impl ModuleInstance {
    pub(super) fn new(
        instance: <WasmRuntime as WasmBackend>::Instance,
        code: Option<CompiledCode>,
        calls: Option<Calls>,
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

    fn release_host_write_pages(&mut self, context: &mut Context<'_>) {
        for page in self.host_write_pages.drain(..) {
            page.release(context);
        }
    }
}

impl Drop for ModuleInstance {
    fn drop(&mut self) {
        if let Some(code) = &self.registered_source {
            code.unregister();
        }
    }
}

pub(super) struct InstalledModules {
    pub(super) dispatch: DispatchTable,
    modules: BTreeMap<ModuleId, ModuleInstance>,
    next_module_id: usize,
    by_linear_page: BTreeMap<u32, BTreeSet<ModuleId>>,
}

impl InstalledModules {
    pub(super) fn new(dispatch: DispatchTable) -> Self {
        Self {
            dispatch,
            modules: BTreeMap::new(),
            next_module_id: 0,
            by_linear_page: BTreeMap::new(),
        }
    }

    pub(super) fn register_exports(
        &mut self,
        wasm: &mut WasmRuntime,
        mut module: ModuleInstance,
        entries: &[DispatchEntry<'_>],
    ) -> Result<(ModuleId, &mut ModuleInstance)> {
        let id = ModuleId(self.next_module_id);
        module.entry_keys = self
            .dispatch
            .register_exports(wasm, id, &module.instance, entries)?;
        if let Some(calls) = &module.calls {
            self.dispatch.register_calls(wasm, id, calls)?;
        }
        for &page in &module.linear_pages {
            self.by_linear_page.entry(page).or_default().insert(id);
        }
        self.next_module_id = self
            .next_module_id
            .checked_add(1)
            .expect("module identity fits usize");
        Ok((id, self.modules.entry(id).insert_entry(module).into_mut()))
    }

    /// Retire each complete linked version that covers a changed byte.
    pub(super) fn invalidate(
        &mut self,
        context: &mut Context<'_>,
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
        self.retire(context, retired_modules)
    }

    pub(super) fn retire(
        &mut self,
        context: &mut Context<'_>,
        retired_modules: BTreeSet<ModuleId>,
    ) -> Result<()> {
        for id in &retired_modules {
            if let Some(calls) = &self.modules[id].calls {
                self.dispatch.unregister_calls(*id, calls);
            }
        }
        let exports = retired_modules.iter().flat_map(|&id| {
            self.modules[&id]
                .entry_keys
                .iter()
                .map(move |&key| (id, key))
        });
        self.dispatch.remove(context, exports)?;
        for id in retired_modules {
            let mut module = self.modules.remove(&id).expect("installed module");
            module.release_host_write_pages(context);
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

    pub(super) fn covering(&self, address: u32) -> impl Iterator<Item = &ModuleInstance> {
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
