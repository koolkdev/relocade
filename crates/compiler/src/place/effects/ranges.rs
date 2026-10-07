//! Local access ranges retain relative addresses; call summaries erase local IDs.
use std::ops::Range;

use crate::{
    body::{FunctionGraph, ValueDefinition},
    memory::{Location, Mem},
};

#[derive(Clone, Eq, PartialEq)]
pub(in crate::place) struct MemoryRange {
    memory: Mem,
    base: Option<usize>,
    bytes: Option<Range<u64>>,
}

impl MemoryRange {
    fn new(
        memory: Mem,
        base: usize,
        offset: u64,
        bytes: Option<u64>,
        body: &FunctionGraph,
    ) -> Self {
        let base = body.values.representation(base);
        let (base, start) = match body.values[base].definition {
            ValueDefinition::Constant(address) => (None, address + offset),
            _ => (Some(base), offset),
        };
        Self {
            memory,
            base,
            bytes: bytes.map(|bytes| start..start + bytes),
        }
    }

    pub(super) fn from_location(location: Location, body: &FunctionGraph) -> Self {
        Self::new(
            location.memory,
            location.base,
            u64::from(location.offset),
            Some(u64::from(location.bytes)),
            body,
        )
    }

    pub(super) fn overlaps(&self, other: &Self) -> bool {
        if self.memory != other.memory {
            return false;
        }
        if self.bytes.as_ref().is_some_and(Range::is_empty)
            || other.bytes.as_ref().is_some_and(Range::is_empty)
        {
            return false;
        }
        match (&self.bytes, &other.bytes) {
            // Equal local bases preserve disjoint offsets, just as absolute
            // addresses do. Different unknown bases can still alias.
            (Some(a), Some(b)) if self.base == other.base => a.start < b.end && b.start < a.end,
            _ => true,
        }
    }

    pub(super) fn for_caller(mut self) -> Self {
        if self.base.take().is_some() && !self.bytes.as_ref().is_some_and(Range::is_empty) {
            self.bytes = None;
        }
        self
    }
}
