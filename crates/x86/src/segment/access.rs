//! Segment permissions and complete offset spans are checked before paging.

use relocade_compiler::{BlockBuilder, BuildError, Val, I1, I32};

use crate::{exception::ExceptionValues, memory::Intent, ssa::StateFields, state};

use super::{Segment, SegmentSelection};

mod fields;

#[cfg(test)]
mod tests;

/// Segment reads and access rules over the current path's retained CPU fields.
pub(crate) struct SegmentAccess<'state> {
    fields: &'state mut StateFields,
}

impl<'state> SegmentAccess<'state> {
    pub(crate) fn new(fields: &'state mut StateFields) -> Self {
        Self { fields }
    }
}

/// A non-faulting segment probe. A proven span needs no runtime guard.
pub(crate) struct SegmentCheck {
    pub(crate) linear: Val<I32>,
    pub(crate) denied: Option<Val<I1>>,
}

impl SegmentAccess<'_> {
    /// Known flat data segments need no cache reads or segment guards.
    /// Unknown indices use the complete checked path.
    /// The span must be nonempty.
    pub(crate) fn translate(
        &mut self,
        body: &mut BlockBuilder<'_>,
        segment: &SegmentSelection,
        offset: &Val<I32>,
        bytes: impl Into<Val<I32>>,
        intent: Intent,
        on_fault: impl Fn(BlockBuilder<'_>, ExceptionValues) -> Result<(), BuildError>,
    ) -> Result<Val<I32>, BuildError> {
        let bytes = body.value(bytes)?;
        assert_ne!(
            body.constant_bits(&bytes)?,
            Some(0),
            "a segment span must be nonempty"
        );
        let check = self.check(body, segment, offset, bytes, intent)?;
        check.translate(body, segment, on_fault)
    }

    /// Probes permissions and an offset span without raising an exception.
    /// Fetch windows may fall back to smaller reads; transfers only need the
    /// target predicate, without checking its page or using its linear address.
    /// Empty spans are denied, as for non-faulting memory probes.
    pub(crate) fn check(
        &mut self,
        body: &mut BlockBuilder<'_>,
        segment: &SegmentSelection,
        offset: &Val<I32>,
        bytes: impl Into<Val<I32>>,
        intent: Intent,
    ) -> Result<SegmentCheck, BuildError> {
        let bytes = body.value(bytes)?;
        let base = self.base(body, segment)?;
        let limit = self.limit(body, segment)?;
        let protected = state::protected_mode(body, self.fields)?;
        let allowed = protected.select(
            self.check_protected_mode(body, segment, intent, &limit, offset, &bytes)?,
            check_real_mode(&limit, offset, &bytes),
        );
        let denied = bytes.eq(0).or(allowed.eq(false));
        Ok(SegmentCheck {
            linear: base.add(offset),
            denied: (body.constant_bits(&denied)? != Some(0)).then_some(denied),
        })
    }

    fn check_protected_mode(
        &mut self,
        body: &mut BlockBuilder<'_>,
        segment: &SegmentSelection,
        intent: Intent,
        limit: &Val<I32>,
        offset: &Val<I32>,
        bytes: &Val<I32>,
    ) -> Result<Val<I1>, BuildError> {
        let usable = self.usable(body, segment)?;
        let permitted = match intent {
            Intent::Read => self.readable(body, segment)?,
            Intent::ReadOrExecute => self
                .readable(body, segment)?
                .or(self.executable(body, segment)?),
            Intent::Write => self.writable(body, segment)?,
            Intent::Fetch => self.executable(body, segment)?,
        };
        let last = offset.add(bytes.sub(1));
        let no_wrap = last.unsigned().ge(offset);
        let upper = self.big(body, segment)?.select(u32::MAX, 0xffffu32);
        let in_bounds = self.expand_down(body, segment)?.select(
            limit
                .unsigned()
                .lt(offset)
                .and(&no_wrap)
                .and(upper.unsigned().ge(&last)),
            // A full-size protected expand-up segment permits wrapping offsets.
            limit
                .eq(u32::MAX)
                .or(no_wrap.and(limit.unsigned().ge(&last))),
        );
        Ok(usable.and(permitted).and(in_bounds))
    }
}

// Compare against the last legal start so a wrapping end cannot pass the check.
fn check_real_mode(limit: &Val<I32>, offset: &Val<I32>, bytes: &Val<I32>) -> Val<I1> {
    let tail_bytes = bytes.sub(1);
    limit
        .unsigned()
        .ge(&tail_bytes)
        .and(limit.sub(tail_bytes).unsigned().ge(offset))
}

impl SegmentCheck {
    pub(crate) fn translate(
        self,
        body: &mut BlockBuilder<'_>,
        segment: &SegmentSelection,
        on_fault: impl Fn(BlockBuilder<'_>, ExceptionValues) -> Result<(), BuildError>,
    ) -> Result<Val<I32>, BuildError> {
        if let Some(denied) = self.denied {
            body.if_(denied, |mut fault| {
                fault.if_else(
                    segment.index().eq(Segment::Ss as u32),
                    |arm| on_fault(arm, ExceptionValues::stack_fault(0)),
                    |arm| on_fault(arm, ExceptionValues::general_protection(0)),
                )
            })?;
        }
        Ok(self.linear)
    }
}
