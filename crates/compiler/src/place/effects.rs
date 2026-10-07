//! Memory footprints used when placing calls and reads.
mod operations;
mod ranges;
#[cfg(test)]
mod tests;
use operations::Accesses;
use ranges::MemoryRange;

use crate::{
    body::{BlockItem, Exit, FunctionGraph, Operation, OperationKind},
    FunctionKind, Program,
};

pub(super) enum Effects<R = Vec<MemoryRange>> {
    // A host or unresolved recursive call can have observable effects even in a
    // module with no memory declarations. Empty read/write lists cannot express it.
    Unknown,
    Known {
        reads: R,
        writes: R,
        synchronizes: bool,
    },
}

impl<R: AsRef<[MemoryRange]>> Effects<R> {
    pub(super) fn must_execute(&self) -> bool {
        match self {
            Self::Unknown => true,
            Self::Known {
                writes,
                synchronizes,
                ..
            } => *synchronizes || !writes.as_ref().is_empty(),
        }
    }

    fn borrowed(&self) -> Effects<Accesses<'_>> {
        match self {
            Self::Unknown => Effects::Unknown,
            Self::Known {
                reads,
                writes,
                synchronizes,
            } => Effects::Known {
                reads: Accesses::Borrowed(reads.as_ref()),
                writes: Accesses::Borrowed(writes.as_ref()),
                synchronizes: *synchronizes,
            },
        }
    }

    fn blocks_read<S: AsRef<[MemoryRange]>>(&self, reader: &Effects<S>) -> bool {
        let Effects::Known { reads, .. } = reader else {
            return true;
        };
        let reads = reads.as_ref();
        if reads.is_empty() {
            return false;
        }
        match self {
            Self::Unknown => true,
            Self::Known {
                writes,
                synchronizes,
                ..
            } => {
                *synchronizes
                    || writes
                        .as_ref()
                        .iter()
                        .any(|write| reads.iter().any(|read| write.overlaps(read)))
            }
        }
    }
}

pub(super) fn observable(operation: &Operation, summaries: &[Effects]) -> bool {
    match operation.kind() {
        OperationKind::Load { .. } => false,
        OperationKind::Call { target } => summaries[target.0].must_execute(),
        _ => true,
    }
}

pub(super) fn blocks_read(
    writer: &Operation,
    reader: &Operation,
    body: &FunctionGraph,
    summaries: &[Effects],
) -> bool {
    if !observable(writer, summaries) {
        return false;
    }
    operations::describe(writer, body, summaries)
        .blocks_read(&operations::describe(reader, body, summaries))
}

fn include(target: &mut Vec<MemoryRange>, ranges: impl IntoIterator<Item = MemoryRange>) {
    for range in ranges {
        let range = range.for_caller();
        if !target.contains(&range) {
            target.push(range);
        }
    }
}

// Summaries include authored reads even when local result demand can remove them.
// This can restrict motion, but cannot hide a possible memory dependency.
fn summarize(body: &FunctionGraph, summaries: &[Option<Effects>]) -> Option<Effects> {
    let mut reads = Vec::new();
    let mut writes = Vec::new();
    let mut callees = Vec::new();
    let mut synchronizes = false;
    let reachable = body.reachable();
    for (index, block) in body.blocks.iter().enumerate() {
        if !reachable[index] {
            continue;
        }
        for item in &block.items {
            let BlockItem::Effect(effect) = item else {
                continue;
            };
            let operation = &body.effects[effect.0].operation;
            if let OperationKind::Call { target } = operation.kind() {
                callees.push(target);
                continue;
            }
            let Effects::Known {
                reads: operation_reads,
                writes: operation_writes,
                synchronizes: operation_synchronizes,
            } = operations::direct(operation, body)
            else {
                unreachable!("direct operations have known effects")
            };
            synchronizes |= operation_synchronizes;
            include(&mut reads, operation_reads.as_ref().iter().cloned());
            include(&mut writes, operation_writes.as_ref().iter().cloned());
        }
        if let Exit::TailCall { target, .. } = &block.exit {
            callees.push(*target);
        }
    }
    for callee in callees {
        match summaries[callee.0].as_ref()? {
            Effects::Unknown => return Some(Effects::Unknown),
            Effects::Known {
                reads: child_reads,
                writes: child_writes,
                synchronizes: child_synchronizes,
            } => {
                synchronizes |= child_synchronizes;
                include(&mut reads, child_reads.iter().cloned());
                include(&mut writes, child_writes.iter().cloned());
            }
        }
    }
    Some(Effects::Known {
        reads,
        writes,
        synchronizes,
    })
}

pub(super) fn infer(program: &Program) -> Vec<Effects> {
    let mut summaries: Vec<_> = program
        .functions
        .iter()
        .map(|function| {
            matches!(function.kind, FunctionKind::Imported { .. }).then_some(Effects::Unknown)
        })
        .collect();
    loop {
        let mut changed = false;
        for (id, function) in program.functions.iter().enumerate() {
            if summaries[id].is_some() {
                continue;
            }
            let FunctionKind::Defined(Some(body)) = &function.kind else {
                unreachable!("inference requires completed definitions")
            };
            if let Some(summary) = summarize(body, &summaries) {
                summaries[id] = Some(summary);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    // Cycles and callers depending on them remain conservative; no promise of
    // returning or lack of effects is inferred from an unresolved call graph.
    summaries
        .into_iter()
        .map(|summary| summary.unwrap_or(Effects::Unknown))
        .collect()
}
