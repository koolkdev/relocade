//! Known calls tail-call a constant slot owned by their installed module.

#[cfg(test)]
mod tests;

use super::ExecutionBuilder;
use crate::abi;
use relocade_compiler::{BlockBuilder, BuildError, Program, Table};
use std::collections::BTreeMap;

#[derive(Clone)]
pub(super) struct CallLinks {
    table: Table,
    sites: BTreeMap<u32, u32>,
}

impl CallLinks {
    pub(super) fn declare(program: &mut Program) -> Self {
        Self {
            table: program.import_table(abi::CALL_TARGETS),
            sites: BTreeMap::new(),
        }
    }

    pub(super) fn for_function(&self, sites: BTreeMap<u32, u32>) -> Self {
        Self {
            table: self.table,
            sites,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct CallLink {
    table: Table,
    index: u32,
}

impl CallLink {
    pub(super) fn enter(self, body: BlockBuilder<'_>) -> Result<(), BuildError> {
        body.tail_call_indirect(self.table, self.index, &[])
    }
}

impl ExecutionBuilder<'_, '_> {
    pub(super) fn call_link(&self) -> Result<Option<CallLink>, BuildError> {
        let index = self
            .body
            .constant_bits(&self.eip)?
            .and_then(|eip| self.call_links.sites.get(&(eip as u32)));
        Ok(index.map(|&index| CallLink {
            table: self.call_links.table,
            index,
        }))
    }
}
