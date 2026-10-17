//! Segment permissions and complete offset spans are checked before paging.

use wasm86_compiler::{BlockBuilder, BuildError, Val, I1, I32};

use crate::{exception::Exception, memory::Intent, state::SegmentReads, ExecutionProfile};

use super::{Segment, SegmentSelection};

#[cfg(test)]
mod tests;

/// A non-faulting segment probe. A profile-proven span needs no runtime guard.
pub(crate) struct SegmentCheck {
    pub(crate) linear: Val<I32>,
    pub(crate) denied: Option<Val<I1>>,
}

impl SegmentReads<'_> {
    /// The entry's profile must remain compatible until a terminal segment load.
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
        on_fault: impl Fn(BlockBuilder<'_>, Exception<Val<I32>>) -> Result<(), BuildError>,
    ) -> Result<Val<I32>, BuildError> {
        let check = self.check(body, segment, offset, bytes, intent)?;
        if let Some(denied) = check.denied {
            body.if_(denied, |mut fault| {
                fault.if_else(
                    segment.index().eq(Segment::Ss as u32),
                    |arm| {
                        on_fault(
                            arm,
                            Exception::StackFault {
                                error_code: 0.into(),
                            },
                        )
                    },
                    |arm| {
                        on_fault(
                            arm,
                            Exception::GeneralProtection {
                                error_code: 0.into(),
                            },
                        )
                    },
                )
            })?;
        }
        Ok(check.linear)
    }

    /// Probes permissions and an offset span without raising an exception.
    /// Fetch windows may fall back to smaller reads; transfers only need the
    /// target predicate, without checking its page or using its linear address.
    /// The span must be nonempty.
    pub(crate) fn check(
        &mut self,
        body: &mut BlockBuilder<'_>,
        segment: &SegmentSelection,
        offset: &Val<I32>,
        bytes: impl Into<Val<I32>>,
        intent: Intent,
    ) -> Result<SegmentCheck, BuildError> {
        let bytes = body.value(bytes)?;
        assert_ne!(
            body.constant_bits(&bytes)?,
            Some(0),
            "a segment span must be nonempty"
        );
        let base = self.base(body, segment)?;
        let denied = match self.profile() {
            ExecutionProfile::Real16 => {
                // Canonical real-mode limits apply without protected-mode type permissions.
                bytes
                    .unsigned()
                    .ge(0x10001u32)
                    .or(offset.unsigned().ge(Val::<I32>::from(0x10001).sub(&bytes)))
            }
            ExecutionProfile::Protected(_) => {
                self.access_denied(body, segment, offset, &bytes, intent)?
            }
        };
        Ok(SegmentCheck {
            linear: base.add(offset),
            denied: (body.constant_bits(&denied)? != Some(0)).then_some(denied),
        })
    }

    fn access_denied(
        &mut self,
        body: &mut BlockBuilder<'_>,
        segment: &SegmentSelection,
        offset: &Val<I32>,
        bytes: &Val<I32>,
        intent: Intent,
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
        let permitted = usable.and(permitted);
        let limit = self.limit(body, segment)?;
        let expand_down = self.expand_down(body, segment)?;
        // A known full-size expand-up segment has no offset restriction. Avoid
        // constructing range expressions or reading its unused D/B field.
        if body.constant_bits(&expand_down)? == Some(0)
            && body.constant_bits(&limit)? == Some(u64::from(u32::MAX))
        {
            return Ok(permitted.eq(false));
        }
        let last = offset.add(bytes.sub(1));
        let no_wrap = last.unsigned().ge(offset);
        // Full-size expand-up segments permit wrapping offsets in this emulator.
        // Finite limits must cover every byte without offset arithmetic wrapping.
        let expand_up = limit
            .eq(u32::MAX)
            .or(no_wrap.and(limit.unsigned().ge(&last)));
        let upper = self.big(body, segment)?.select(u32::MAX, 0xffffu32);
        let down_range = limit
            .unsigned()
            .lt(offset)
            .and(&no_wrap)
            .and(upper.unsigned().ge(&last));
        Ok(permitted
            .and(expand_down.select(down_range, expand_up))
            .eq(0))
    }
}
