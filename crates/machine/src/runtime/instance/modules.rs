//! CPU-local module instances, callable exports and decoded guest-code coverage.

use super::table::DispatchTable;
use crate::{CompiledCode, DispatchEntry, Result};
use relocade_wasm_runtime::ExportFunction;
use relocade_x86::abi;
use std::collections::{BTreeMap, BTreeSet};

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

    pub(super) fn publish(&self, backend: &mut B, table: &mut DispatchTable<B>) -> Result<()> {
        let entries: Vec<_> = self
            .entries
            .iter()
            .map(|(&key, export)| DispatchEntry { key, export })
            .collect();
        table.install(backend, &self.instance, &entries)
    }
}

pub(super) struct Modules<B: ExportFunction<abi::ExecutionSignature>> {
    modules: Vec<InstalledModule<B>>,
    pages: BTreeMap<u32, Vec<usize>>,
}

impl<B: ExportFunction<abi::ExecutionSignature>> Default for Modules<B> {
    fn default() -> Self {
        Self {
            modules: Vec::new(),
            pages: BTreeMap::new(),
        }
    }
}

impl<B: ExportFunction<abi::ExecutionSignature>> Modules<B> {
    pub(super) fn insert(&mut self, module: InstalledModule<B>) {
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
                    code.ranges()
                        .iter()
                        .any(|range| range.contains(&u64::from(address)))
                })
            })
    }
}
