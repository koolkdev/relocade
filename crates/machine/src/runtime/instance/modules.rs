//! CPU-local module instances, callable exports and decoded guest-code coverage.

mod calls;
pub(super) use calls::Calls;
mod table;

use crate::memory::compiled::CodeWatch;
use crate::{CompiledCode, DispatchEntry, FarPointer, Result};
use relocade_wasm_runtime::{ExportFunction, Import};
use relocade_x86::abi;
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
    sync::Arc,
};
use table::DispatchTable;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ModuleId(usize);

pub(super) struct InstalledModule<B: ExportFunction<abi::ExecutionSignature>> {
    instance: B::Instance,
    entries: BTreeMap<u32, String>,
    code: Option<CompiledCode>,
    calls: Option<Calls<B>>,
    pages: BTreeSet<u32>,
    watch: Option<Arc<CodeWatch>>,
}

impl<B: ExportFunction<abi::ExecutionSignature>> InstalledModule<B> {
    pub(super) fn new(
        instance: B::Instance,
        entries: &[DispatchEntry<'_>],
        code: Option<CompiledCode>,
        calls: Option<Calls<B>>,
        watch: Option<Arc<CodeWatch>>,
    ) -> Self {
        let pages = code
            .iter()
            .flat_map(|code| code.ranges())
            .flat_map(|range| (range.start >> 12)..=((range.end - 1) >> 12))
            .map(|page| page as u32)
            .collect();
        Self {
            instance,
            entries: entries
                .iter()
                .map(|entry| (entry.key, entry.export.to_owned()))
                .collect(),
            code,
            calls,
            pages,
            watch,
        }
    }

    pub(super) fn code(&self) -> Option<&CompiledCode> {
        self.code.as_ref()
    }

    fn publish(&self, backend: &mut B, table: &mut DispatchTable<B>, id: ModuleId) -> Result<()> {
        let entries: Vec<_> = self
            .entries
            .iter()
            .map(|(&key, export)| DispatchEntry { key, export })
            .collect();
        table.install(backend, id, &self.instance, &entries)
    }
}

pub(super) struct Modules<B: ExportFunction<abi::ExecutionSignature>> {
    table: DispatchTable<B>,
    modules: BTreeMap<ModuleId, InstalledModule<B>>,
    next_module_id: usize,
    pages: BTreeMap<u32, BTreeSet<ModuleId>>,
}

impl<B: ExportFunction<abi::ExecutionSignature>> Modules<B> {
    pub(super) fn new(backend: &mut B) -> Result<Self> {
        Ok(Self {
            table: DispatchTable::new(backend)?,
            modules: BTreeMap::new(),
            next_module_id: 0,
            pages: BTreeMap::new(),
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
            && !self
                .table
                .owner(key)
                .and_then(|id| self.modules[&id].watch.as_ref())
                .is_some_and(|watch| watch.changed())
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

    pub(super) fn install(&mut self, backend: &mut B, module: InstalledModule<B>) -> Result<()> {
        let id = ModuleId(self.next_module_id);
        module.publish(backend, &mut self.table, id)?;
        if let Some(calls) = &module.calls {
            self.table.register_calls(backend, id, calls)?;
        }
        for &page in &module.pages {
            self.pages.entry(page).or_default().insert(id);
        }
        self.modules.insert(id, module);
        self.next_module_id = self
            .next_module_id
            .checked_add(1)
            .expect("module identity fits usize");
        Ok(())
    }

    /// Retires complete linked versions: their direct internal calls cannot be
    /// changed by updating only the entry for the modified block.
    pub(super) fn invalidate(&mut self, backend: &mut B, range: Range<u64>) -> Result<()> {
        if range.is_empty() {
            return Ok(());
        }
        let mut retired_modules: BTreeSet<_> = self
            .pages
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
        self.retire(backend, retired_modules).map(|_| ())
    }

    pub(super) fn retire_changed(&mut self, backend: &mut B) -> Result<Vec<u32>> {
        let retired = self
            .modules
            .iter()
            .filter_map(|(&id, module)| {
                module
                    .watch
                    .as_ref()
                    .is_some_and(|watch| watch.changed())
                    .then_some(id)
            })
            .collect();
        self.retire(backend, retired)
    }

    fn retire(&mut self, backend: &mut B, retired_modules: BTreeSet<ModuleId>) -> Result<Vec<u32>> {
        for id in &retired_modules {
            if let Some(calls) = &self.modules[id].calls {
                self.table.unregister_calls(*id, calls);
            }
        }
        let exports = retired_modules
            .iter()
            .flat_map(|&id| self.modules[&id].entries.keys().map(move |&key| (id, key)));
        let keys = self.table.remove(backend, exports)?;
        for id in retired_modules {
            let module = self.modules.remove(&id).expect("installed module");
            for page in module.pages {
                let modules = self.pages.get_mut(&page).expect("registered source page");
                modules.remove(&id);
                if modules.is_empty() {
                    self.pages.remove(&page);
                }
            }
        }
        Ok(keys)
    }

    pub(super) fn covering(&self, address: u32) -> impl Iterator<Item = &InstalledModule<B>> {
        self.pages
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
