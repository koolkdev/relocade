//! Validates captured instruction bytes immediately before publishing code.

use super::{code::CompiledEntry, Backend, Dispatch, DispatchEntry, RuntimeMemory};
use crate::{CompiledCode, Memory, Result};
use relocade_x86::PhysicalMapping;
use std::collections::BTreeSet;

impl Dispatch {
    pub(crate) async fn install_code(
        &mut self,
        backend: &mut Backend,
        bindings: &RuntimeMemory,
        code: &CompiledCode,
        cs_base: u32,
    ) -> Result<bool> {
        if code.execution_target != bindings.configuration().execution_target()
            || cs_base > 0xffff0
            || cs_base & 15 != 0
        {
            return Ok(false);
        }
        let instance = self.instantiate(backend, bindings, &code.module).await?;
        self.table.sync(backend, &self.interpret);
        let Some(dependencies) = dependencies(code, &bindings.guest, cs_base)? else {
            return Ok(false);
        };
        {
            let registry = self.table.code.lock().unwrap();
            if dependencies.iter().any(|entry| registry.conflicts(entry)) {
                return Ok(false);
            }
        }
        let entries: Vec<_> = code
            .entries
            .iter()
            .map(|entry| DispatchEntry {
                address: cs_base + entry.start_eip,
                export: &entry.export,
            })
            .collect();
        // No await or guest execution between validation and publication. Machine
        // memory permits one writer, including across CPUs sharing its backing.
        self.table.install(backend, &instance, &entries)?;
        let mut registry = self.table.code.lock().unwrap();
        registry.track(dependencies);
        registry.publish(backend);
        Ok(true)
    }
}

fn dependencies(
    code: &CompiledCode,
    memory: &Memory,
    cs_base: u32,
) -> Result<Option<Vec<CompiledEntry>>> {
    let map = memory.configuration().memory_map();
    let view = memory.view();
    let mut dependencies = Vec::with_capacity(code.entries.len());
    for (entry, snapshot) in code.entries.iter().zip(&code.snapshots) {
        if u64::from(entry.start_eip) + entry.byte_len as u64 > 0x10000 {
            return Ok(None);
        }
        let address = cs_base + entry.start_eip;
        let mut pages = BTreeSet::new();
        let mut offset = 0;
        while offset < entry.byte_len {
            let linear = address + offset as u32;
            let backing = match map.get(linear) {
                PhysicalMapping::Ram { backing_offset }
                | PhysicalMapping::Rom { backing_offset } => backing_offset,
                _ => return Ok(None),
            };
            pages.insert(backing & !4095);
            offset += (4096 - (linear & 4095) as usize).min(entry.byte_len - offset);
        }
        let mut bytes = vec![0; entry.byte_len];
        view.read(address, &mut bytes)?;
        if bytes != snapshot.bytes[..entry.byte_len] {
            return Ok(None);
        }
        dependencies.push(CompiledEntry {
            address,
            byte_len: entry.byte_len as u32,
            pages,
        });
    }
    Ok(Some(dependencies))
}
