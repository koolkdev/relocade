//! CPU-local module instances, callable exports and decoded guest-code coverage.

mod calls;
pub(super) use calls::Calls;
mod table;

use crate::{CompiledCode, DispatchEntry, FarPointer, Result};
use relocade_wasm_runtime::{ExportFunction, Import};
use relocade_x86::abi;
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
};
use table::DispatchTable;

pub(super) struct InstalledModule<B: ExportFunction<abi::ExecutionSignature>> {
    instance: B::Instance,
    entries: BTreeMap<u32, String>,
    code: Option<CompiledCode>,
    calls: Option<Calls<B>>,
}

impl<B: ExportFunction<abi::ExecutionSignature>> InstalledModule<B> {
    pub(super) fn new(
        instance: B::Instance,
        entries: &[DispatchEntry<'_>],
        code: Option<CompiledCode>,
        calls: Option<Calls<B>>,
    ) -> Self {
        Self {
            instance,
            entries: entries
                .iter()
                .map(|entry| (entry.key, entry.export.to_owned()))
                .collect(),
            code,
            calls,
        }
    }

    pub(super) fn code(&self) -> Option<&CompiledCode> {
        self.code.as_ref()
    }

    fn publish(&self, backend: &mut B, table: &mut DispatchTable<B>) -> Result<()> {
        let entries: Vec<_> = self
            .entries
            .iter()
            .map(|(&key, export)| DispatchEntry { key, export })
            .collect();
        table.install(backend, &self.instance, &entries)
    }
}

pub(super) struct Modules<B: ExportFunction<abi::ExecutionSignature>> {
    table: DispatchTable<B>,
    modules: BTreeMap<usize, InstalledModule<B>>,
    next_module_id: usize,
    owners: BTreeMap<u32, usize>,
    pages: BTreeMap<u32, Vec<usize>>,
    callers: BTreeMap<u32, Vec<CallSite>>,
}

struct CallSite {
    module: usize,
    index: u32,
}

impl<B: ExportFunction<abi::ExecutionSignature>> Modules<B> {
    pub(super) fn new(backend: &mut B) -> Result<Self> {
        Ok(Self {
            table: DispatchTable::new(backend)?,
            modules: BTreeMap::new(),
            next_module_id: 0,
            owners: BTreeMap::new(),
            pages: BTreeMap::new(),
            callers: BTreeMap::new(),
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
        self.table.install_interpreter_fallback(backend, entry)?;
        self.relink(backend, [entry.packed()])
    }

    pub(super) fn install(&mut self, backend: &mut B, module: InstalledModule<B>) -> Result<()> {
        module.publish(backend, &mut self.table)?;
        let keys: Vec<_> = module.entries.keys().copied().collect();
        let id = self.next_module_id;
        if let Some(calls) = &module.calls {
            for (index, &target) in calls.targets.iter().enumerate() {
                calls.bind(backend, &self.table, index as u32)?;
                self.callers.entry(target).or_default().push(CallSite {
                    module: id,
                    index: index as u32,
                });
            }
        }
        self.owners.extend(keys.iter().map(|&key| (key, id)));
        self.insert(id, module);
        self.next_module_id = id.checked_add(1).expect("module identity fits usize");
        self.relink(backend, keys)
    }

    fn relink(&self, backend: &mut B, keys: impl IntoIterator<Item = u32>) -> Result<()> {
        for key in keys {
            for site in self.callers.get(&key).into_iter().flatten() {
                self.modules[&site.module]
                    .calls
                    .as_ref()
                    .expect("registered caller")
                    .bind(backend, &self.table, site.index)?;
            }
        }
        Ok(())
    }

    fn insert(&mut self, id: usize, module: InstalledModule<B>) {
        if let Some(code) = module.code() {
            let pages: BTreeSet<_> = code
                .ranges()
                .iter()
                .flat_map(|range| (range.start >> 12)..=((range.end - 1) >> 12))
                .collect();
            for page in pages {
                self.pages.entry(page as u32).or_default().push(id);
            }
        }
        self.modules.insert(id, module);
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
        if retired_modules.is_empty() {
            return Ok(());
        }
        let mut keys = Vec::new();
        for id in &retired_modules {
            for &key in self.modules[id].entries.keys() {
                if self.owners.get(&key) == Some(id) {
                    keys.push(key);
                }
            }
        }
        self.table.remove(backend, &keys)?;
        for key in &keys {
            self.owners.remove(key);
        }
        self.callers.retain(|_, sites| {
            sites.retain(|site| !retired_modules.contains(&site.module));
            !sites.is_empty()
        });
        self.relink(backend, keys)?;
        self.pages.retain(|_, modules| {
            modules.retain(|id| !retired_modules.contains(id));
            !modules.is_empty()
        });
        for id in retired_modules {
            self.modules.remove(&id);
        }
        Ok(())
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
