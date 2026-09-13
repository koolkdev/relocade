//! Instruction reads translate CS offsets before consulting linear page mappings.

use wasm86_compiler::{BuildError, FunctionBuilder, Val, I32, I8};

use crate::{
    memory::{DirectRange, Intent, Memory},
    segment::{Segment, SegmentAccess, SegmentProfile},
    state::{exit, Cpu},
};

#[derive(Clone, Copy)]
pub(crate) struct InstructionFetch<'module> {
    pub(super) memory: &'module Memory,
    segments: SegmentAccess<'module>,
}

impl<'module> InstructionFetch<'module> {
    pub(crate) fn new(cpu: &'module Cpu, memory: &'module Memory, profile: SegmentProfile) -> Self {
        Self {
            memory,
            segments: SegmentAccess::new(cpu, profile),
        }
    }

    /// An unavailable window is not a fault: a shorter instruction may still fit.
    pub(super) fn check_direct_access(
        &self,
        body: &mut FunctionBuilder<'_>,
        eip: &Val<I32>,
        bytes: u32,
    ) -> Result<DirectRange, BuildError> {
        let segment = self
            .segments
            .check(body, &Segment::Cs.into(), eip, bytes, Intent::Fetch)?;
        let mut direct =
            self.memory
                .check_direct_access(body, &segment.linear, bytes, Intent::Fetch)?;
        if let Some(denied) = segment.denied {
            direct.unavailable = denied.or(direct.unavailable);
        }
        Ok(direct)
    }

    /// Exact reads check CS first, then the page containing that required byte.
    pub(super) fn byte(
        &self,
        body: &mut FunctionBuilder<'_>,
        eip: &Val<I32>,
    ) -> Result<Val<I8>, BuildError> {
        let linear = self.segments.translate::<I8>(
            body,
            &Segment::Cs.into(),
            eip,
            Intent::Fetch,
            exit::exception,
        )?;
        let access =
            self.memory
                .resolve_access::<I8>(body, &linear, Intent::Fetch, exit::exception)?;
        self.memory.read(body, &access)
    }
}
