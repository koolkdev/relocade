//! CPU-local module instances, callable exports and decoded guest-code coverage.

mod table;

use crate::{CompiledCode, DispatchEntry, FarPointer, Result};
use relocade_wasm_runtime::{ExportFunction, Import};
use relocade_x86::abi;
use std::collections::{BTreeMap, BTreeSet};
use table::DispatchTable;

pub(super) struct InstalledModule<B: ExportFunction<abi::ExecutionSignature>> {
    instance: B::Instance,
    entries: BTreeMap<u32, String>,
    code: Option<CompiledCode>,
}

impl<B: ExportFunction<abi::ExecutionSignature>> InstalledModule<B> {
    pub(super) fn new(
        instance: B::Instance,
        entries: &[DispatchEntry<'_>],
        code: Option<CompiledCode>,
    ) -> Self {
        Self {
            instance,
            entries: entries
                .iter()
                .map(|entry| (entry.key, entry.export.to_owned()))
                .collect(),
            code,
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
    modules: Vec<InstalledModule<B>>,
    pages: BTreeMap<u32, Vec<usize>>,
}

impl<B: ExportFunction<abi::ExecutionSignature>> Modules<B> {
    pub(super) fn new(backend: &mut B) -> Result<Self> {
        Ok(Self {
            table: DispatchTable::new(backend)?,
            modules: Vec::new(),
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
        module.publish(backend, &mut self.table)?;
        self.insert(module);
        Ok(())
    }

    fn insert(&mut self, module: InstalledModule<B>) {
        let id = self.modules.len();
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
        self.modules.push(module);
    }

    pub(super) fn covering(&self, address: u32) -> impl Iterator<Item = &InstalledModule<B>> {
        self.pages
            .get(&(address >> 12))
            .into_iter()
            .flatten()
            .map(|&id| &self.modules[id])
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
