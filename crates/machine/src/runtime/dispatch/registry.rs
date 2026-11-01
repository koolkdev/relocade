//! Publishes execution entries and retains their host handlers by stable ID.

use super::{table::DispatchTable, DispatchEntry};
use crate::runtime::{Entry, Hook, Result};
use relocade_compiler::{abi::Function, Program};
use relocade_wasm_runtime::ExportFunction;
use relocade_x86::{abi, ExecutionExitValues, HostExitValues};
use std::collections::{BTreeMap, HashMap};

pub(super) struct Registry<B: ExportFunction<abi::ExecutionSignature>> {
    pub(super) table: DispatchTable<B>,
    ids: HashMap<u32, u32>,
    hooks: Vec<Option<Hook>>,
}

/// Exports and handlers awaiting publication; no shared replacement lifetime.
pub(super) struct Batch {
    entries: Vec<(u32, String)>,
    handlers: Vec<(u32, Hook)>,
}

impl Batch {
    pub fn module(entries: &[DispatchEntry<'_>]) -> Self {
        let entries: BTreeMap<_, _> = entries
            .iter()
            .map(|entry| (entry.address, entry.export.to_owned()))
            .collect();
        Self {
            entries: entries.into_iter().collect(),
            handlers: Vec::new(),
        }
    }
}

impl<B: ExportFunction<abi::ExecutionSignature>> Registry<B> {
    pub fn new(backend: &mut B) -> Result<Self> {
        Ok(Self {
            table: DispatchTable::new(backend)?,
            ids: HashMap::new(),
            hooks: Vec::new(),
        })
    }

    #[inline]
    pub fn hook(&self, id: u32) -> &Hook {
        self.hooks
            .get(id as usize)
            .and_then(Option::as_ref)
            .expect("registered host service")
    }

    pub fn prepare(&self, entries: impl IntoIterator<Item = Entry>) -> Result<(Program, Batch)> {
        // The last declaration at an address wins, for host and guest entries alike.
        let entries: BTreeMap<_, _> = entries
            .into_iter()
            .map(|entry| (entry.address(), entry))
            .collect();
        let mut program = Program::new();
        let mut next_id = self.hooks.len();
        let mut batch = Batch {
            entries: Vec::new(),
            handlers: Vec::new(),
        };
        for (address, entry) in entries {
            let function = match entry {
                Entry::Host(hook) => {
                    let id = self.ids.get(&address).copied().unwrap_or_else(|| {
                        let id = u32::try_from(next_id).expect("host service ID capacity");
                        next_id += 1;
                        id
                    });
                    batch.handlers.push((id, hook));
                    program
                        .function::<abi::ExecutionSignature>(move |body, ()| {
                            body.return_(ExecutionExitValues::host(HostExitValues::call(id)))
                        })
                        .into()
                }
                Entry::Block { build, .. } => build(&mut program),
            };
            let name = format!("entry_{}", batch.entries.len());
            Function::<abi::ExecutionSignature>::new("", &name).export(&mut program, function)?;
            batch.entries.push((address, name));
        }
        Ok((program, batch))
    }

    pub fn publish(&mut self, backend: &mut B, instance: &B::Instance, batch: Batch) -> Result<()> {
        let entries: Vec<_> = batch
            .entries
            .iter()
            .map(|(address, export)| DispatchEntry {
                address: *address,
                export,
            })
            .collect();
        // Export resolution and capacity checks precede replacement. Clear only
        // handlers at overwritten addresses, after their callable slots change.
        self.table.install(backend, instance, &entries)?;
        for entry in entries {
            if let Some(&id) = self.ids.get(&entry.address) {
                self.hooks[id as usize] = None;
            }
        }
        for (id, hook) in batch.handlers {
            self.ids.insert(hook.address, id);
            if self.hooks.len() <= id as usize {
                self.hooks.resize_with(id as usize + 1, || None);
            }
            self.hooks[id as usize] = Some(hook);
        }
        Ok(())
    }
}
