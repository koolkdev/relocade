//! Allocation-free effect descriptions for individual operations.
use super::{Effects, MemoryRange};
use crate::body::{FunctionGraph, Operation, OperationKind};

// Direct operations keep their single access inline; calls borrow summary slices.
pub(super) enum Accesses<'a> {
    One(MemoryRange),
    Borrowed(&'a [MemoryRange]),
}

impl AsRef<[MemoryRange]> for Accesses<'_> {
    fn as_ref(&self) -> &[MemoryRange] {
        match self {
            Self::One(range) => std::slice::from_ref(range),
            Self::Borrowed(ranges) => ranges,
        }
    }
}

pub(super) fn direct(operation: &Operation, body: &FunctionGraph) -> Effects<Accesses<'static>> {
    let mut reads = Accesses::Borrowed(&[]);
    let mut writes = Accesses::Borrowed(&[]);
    let mut synchronizes = false;
    match operation.kind() {
        OperationKind::Load { .. } => {
            reads = Accesses::One(MemoryRange::from_location(
                operation.location().expect("a load has a memory location"),
                body,
            ));
        }
        OperationKind::Store { .. } => {
            writes = Accesses::One(MemoryRange::from_location(
                operation.location().expect("a store has a memory location"),
                body,
            ));
        }
        OperationKind::MemoryFill { memory } => {
            let mut inputs = operation.inputs();
            let destination = inputs.next().unwrap();
            let bytes = inputs.next_back().unwrap();
            writes = Accesses::One(MemoryRange::from_span(memory, destination, bytes, body));
        }
        OperationKind::MemoryCopy {
            destination_memory,
            source_memory,
        } => {
            let mut inputs = operation.inputs();
            let destination = inputs.next().unwrap();
            let source = inputs.next().unwrap();
            let bytes = inputs.next().unwrap();
            reads = Accesses::One(MemoryRange::from_span(source_memory, source, bytes, body));
            writes = Accesses::One(MemoryRange::from_span(
                destination_memory,
                destination,
                bytes,
                body,
            ));
        }
        OperationKind::Atomic { .. } | OperationKind::Fence => synchronizes = true,
        OperationKind::Call { .. } => unreachable!("calls use their function's effect summary"),
    }
    Effects::Known {
        reads,
        writes,
        synchronizes,
    }
}

pub(super) fn describe<'a>(
    operation: &Operation,
    body: &FunctionGraph,
    summaries: &'a [Effects],
) -> Effects<Accesses<'a>> {
    match operation.kind() {
        OperationKind::Call { target } => summaries[target.0].borrowed(),
        _ => direct(operation, body),
    }
}
