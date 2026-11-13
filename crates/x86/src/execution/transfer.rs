//! Completed x86 transfers choose local entries or the shared runtime path.

use super::ExecutionBuilder;
use crate::runtime::Runtime;
use relocade_compiler::{BlockBuilder, BuildError, Func, Val, I1, I32};
use std::collections::BTreeMap;

#[derive(Clone)]
pub(super) enum Transfer {
    Jump,
    Branch {
        condition: Val<I1>,
        taken: Val<I32>,
        fallthrough: Val<I32>,
    },
    Call,
    FarCall,
    Return,
    Dispatch,
}

impl ExecutionBuilder<'_, '_> {
    /// Publishes completed effects and checks execution controls before any link.
    pub(crate) fn dispatch(mut self) -> Result<(), BuildError> {
        self.state
            .publish(&mut self.body, &self.eip, self.completed)?;
        self.checkpoint()?;
        match self.transfer {
            Transfer::Jump => jump(self.body, &self.eip, self.entries, self.runtime),
            Transfer::Branch {
                condition,
                taken,
                fallthrough,
            } => {
                let taken = local_entry(&self.body, &taken, self.entries)?;
                let fallthrough = local_entry(&self.body, &fallthrough, self.entries)?;
                if taken.is_none() && fallthrough.is_none() {
                    return self.runtime.dispatch(self.body);
                }
                self.body
                    .if_(condition, |body| enter(body, taken, self.runtime))?;
                enter(self.body, fallthrough, self.runtime)
            }
            Transfer::Call | Transfer::FarCall => {
                if matches!(self.transfer, Transfer::Call) {
                    if let Some(function) = local_entry(&self.body, &self.eip, self.entries)? {
                        return self.body.tail_call(function, &[]);
                    }
                }
                // Checkpoint exits skip the resolver, so profiling here can miss
                // a completed call. Hotness counts are intentionally approximate.
                let (slot, can_continue) = self.runtime.resolve_call(&mut self.body)?;
                self.body.if_(can_continue.eq(false), |body| {
                    body.return_(crate::ExecutionExitValues::call_profile_full())
                })?;
                self.runtime.enter(self.body, slot)
            }
            Transfer::Return | Transfer::Dispatch => self.runtime.dispatch(self.body),
        }
    }
}

fn local_entry(
    body: &BlockBuilder<'_>,
    eip: &Val<I32>,
    entries: &BTreeMap<u32, Func>,
) -> Result<Option<Func>, BuildError> {
    Ok(body
        .constant_bits(eip)?
        .and_then(|eip| entries.get(&(eip as u32)).copied()))
}

fn jump(
    body: BlockBuilder<'_>,
    eip: &Val<I32>,
    entries: &BTreeMap<u32, Func>,
    runtime: Runtime,
) -> Result<(), BuildError> {
    let function = local_entry(&body, eip, entries)?;
    enter(body, function, runtime)
}

fn enter(
    body: BlockBuilder<'_>,
    function: Option<Func>,
    runtime: Runtime,
) -> Result<(), BuildError> {
    match function {
        Some(function) => body.tail_call(function, &[]),
        None => runtime.dispatch(body),
    }
}
