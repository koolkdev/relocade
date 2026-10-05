//! Instruction reads translate CS offsets before consulting linear page mappings.

use wasm86_compiler::{BlockBuilder, BuildError, Val, I1, I32, I8};

use crate::{
    memory::{Intent, Memory, PageCache},
    segment::{Segment, SegmentAccess, SegmentProfile},
    state::{exit, Cpu},
};

/// A speculative instruction window. Only an available window permits direct reads.
pub(crate) struct FetchWindow {
    pub(crate) unavailable: Val<I1>,
    pub(crate) physical: Val<I32>,
}

#[derive(Clone, Copy)]
pub(crate) struct InstructionFetch<'module> {
    pub(super) memory: &'module Memory,
    cpu: &'module Cpu,
    segments: SegmentAccess<'module>,
}

impl<'module> InstructionFetch<'module> {
    pub(crate) fn new(cpu: &'module Cpu, memory: &'module Memory, profile: SegmentProfile) -> Self {
        Self {
            memory,
            cpu,
            segments: SegmentAccess::new(cpu, profile),
        }
    }

    pub(super) fn eip(&self, body: &mut BlockBuilder<'_>) -> Result<Val<I32>, BuildError> {
        self.cpu.read_eip(body)
    }

    /// An unavailable window is not a fault: a shorter instruction may still fit.
    pub(super) fn probe_window(
        &self,
        body: &mut BlockBuilder<'_>,
        eip: &Val<I32>,
        bytes: u32,
        cache: Option<&mut PageCache>,
    ) -> Result<FetchWindow, BuildError> {
        let segment = self
            .segments
            .check(body, &Segment::Cs.into(), eip, bytes, Intent::Fetch)?;
        let access =
            self.memory
                .resolve_access(body, &segment.linear, bytes, Intent::Fetch, cache, None)?;
        let mut unavailable = access.unavailable;
        if let Some(denied) = segment.denied {
            unavailable = denied.or(unavailable);
        }
        Ok(FetchWindow {
            unavailable,
            physical: access.physical,
        })
    }

    /// Exact reads check CS first, then the page containing that required byte.
    pub(super) fn byte(
        &self,
        body: &mut BlockBuilder<'_>,
        eip: &Val<I32>,
    ) -> Result<Val<I8>, BuildError> {
        let linear = self.segments.translate(
            body,
            &Segment::Cs.into(),
            eip,
            1,
            Intent::Fetch,
            exit::exception,
        )?;
        let access = self.memory.resolve_access(
            body,
            &linear,
            1,
            Intent::Fetch,
            None,
            Some(&mut exit::exception),
        )?;
        self.memory.read(body, &access, 0)
    }
}
