//! Memory footprints used when placing calls and reads.
use std::ops::Range;

use crate::{
    memory::{Location, Mem},
    place, Body, FunctionKind, Operation, Program, Terminal, ValueKind,
};

#[derive(Clone, Eq, PartialEq)]
pub(super) struct MemoryRange {
    memory: Mem,
    bytes: Option<Range<u64>>,
}

impl MemoryRange {
    fn from_location(location: Location, body: &Body) -> Self {
        let base = place::representation(body, location.base);
        let bytes = match body.values[base].kind {
            ValueKind::Constant(base) => {
                let start = base + u64::from(location.offset);
                Some(start..start + u64::from(location.bytes))
            }
            _ => None,
        };
        Self {
            memory: location.memory,
            bytes,
        }
    }

    fn overlaps(&self, other: &Self) -> bool {
        self.memory == other.memory
            && match (&self.bytes, &other.bytes) {
                (Some(a), Some(b)) => a.start < b.end && b.start < a.end,
                _ => true,
            }
    }

    pub(super) fn overlaps_location(&self, location: Location, body: &Body) -> bool {
        self.overlaps(&Self::from_location(location, body))
    }
}

pub(super) enum Effects {
    // A host or unresolved recursive call can have observable effects even in a
    // module with no memory declarations. Empty read/write lists cannot express it.
    Unknown,
    Known {
        reads: Vec<MemoryRange>,
        writes: Vec<MemoryRange>,
    },
}

impl Effects {
    pub(super) fn must_execute(&self) -> bool {
        match self {
            Self::Unknown => true,
            Self::Known { writes, .. } => !writes.is_empty(),
        }
    }

    pub(super) fn writes_location(&self, location: Location, body: &Body) -> bool {
        match self {
            Self::Unknown => true,
            Self::Known { writes, .. } => writes
                .iter()
                .any(|range| range.overlaps_location(location, body)),
        }
    }

    pub(super) fn writes_reads(&self, reads: &[MemoryRange]) -> bool {
        match self {
            Self::Unknown => !reads.is_empty(),
            Self::Known { writes, .. } => writes
                .iter()
                .any(|write| reads.iter().any(|read| write.overlaps(read))),
        }
    }
}

fn include(target: &mut Vec<MemoryRange>, ranges: impl IntoIterator<Item = MemoryRange>) {
    for range in ranges {
        if !target.contains(&range) {
            target.push(range);
        }
    }
}

// Summaries include authored reads even when local result demand can remove them.
// This can restrict motion, but cannot hide a possible memory dependency.
fn summarize(body: &Body, summaries: &[Option<Effects>]) -> Option<Effects> {
    let mut reads = Vec::new();
    let mut writes = Vec::new();
    let mut callees = Vec::new();
    for region in body.region.walk() {
        for operation in &region.operations {
            match operation {
                Operation::Load(value) => {
                    let ValueKind::Load { location, .. } = body.values[*value].kind else {
                        unreachable!("a load operation names its load value")
                    };
                    include(&mut reads, [MemoryRange::from_location(location, body)]);
                }
                Operation::Store { location, .. } => {
                    include(&mut writes, [MemoryRange::from_location(*location, body)])
                }
                Operation::Call { invocation, .. } => callees.push(invocation.target),
                Operation::Block { .. } | Operation::If { .. } | Operation::Switch { .. } => {}
            }
        }
        if let Some(Terminal::TailCall(invocation)) = &region.terminal {
            callees.push(invocation.target);
        }
    }
    for callee in callees {
        match summaries[callee.0].as_ref()? {
            Effects::Unknown => return Some(Effects::Unknown),
            Effects::Known {
                reads: child_reads,
                writes: child_writes,
            } => {
                include(&mut reads, child_reads.iter().cloned());
                include(&mut writes, child_writes.iter().cloned());
            }
        }
    }
    Some(Effects::Known { reads, writes })
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
