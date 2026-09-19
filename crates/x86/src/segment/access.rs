//! Segment permissions and complete offset spans are checked before paging.

use wasm86_compiler::{BuildError, FunctionBuilder, Val, I1, I32};

use crate::{exception::Exception, memory::Intent, state::Cpu};

use super::{
    Segment, SegmentAttributes, SegmentDefaultSize, SegmentProfile, SegmentSelection, SegmentValues,
};

#[cfg(test)]
mod tests;

/// A non-faulting segment probe. A profile-proven span needs no runtime guard.
pub(crate) struct SegmentCheck {
    pub(crate) linear: Val<I32>,
    pub(crate) denied: Option<Val<I1>>,
}

#[derive(Clone, Copy)]
pub(crate) struct SegmentAccess<'cpu> {
    cpu: &'cpu Cpu,
    profile: SegmentProfile,
}

impl<'cpu> SegmentAccess<'cpu> {
    pub(crate) fn new(cpu: &'cpu Cpu, profile: SegmentProfile) -> Self {
        Self { cpu, profile }
    }

    /// Returns the D/B bit, using the profile when it proves the attribute.
    pub(crate) fn is_segment_big(
        &self,
        body: &mut FunctionBuilder<'_>,
        segment: Segment,
    ) -> Result<Val<I1>, BuildError> {
        match segment {
            Segment::Cs => body.value(u32::from(
                self.profile.code_default_size() == SegmentDefaultSize::Bits32,
            )),
            Segment::Ss if self.profile == SegmentProfile::Flat32 => body.value(1),
            _ => Ok(self
                .cpu
                .read_segment(body, &segment.into())?
                .bit(SegmentAttributes::DEFAULT_BIG)),
        }
    }

    /// The entry's profile must remain compatible until a terminal segment load.
    /// Flat address defaults and named data segments need no cache reads or
    /// segment guards. Explicit runtime overrides use the complete checked path.
    pub(crate) fn translate(
        &self,
        body: &mut FunctionBuilder<'_>,
        segment: &SegmentSelection,
        offset: &Val<I32>,
        bytes: u32,
        intent: Intent,
        on_fault: impl Fn(FunctionBuilder<'_>, Exception<Val<I32>>) -> Result<(), BuildError>,
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
    pub(crate) fn check(
        &self,
        body: &mut FunctionBuilder<'_>,
        segment: &SegmentSelection,
        offset: &Val<I32>,
        bytes: u32,
        intent: Intent,
    ) -> Result<SegmentCheck, BuildError> {
        assert!(bytes > 0);
        match (self.profile, segment, intent) {
            (
                SegmentProfile::Flat32,
                SegmentSelection::Named(Segment::Cs),
                Intent::Read | Intent::Fetch,
            ) => Ok(SegmentCheck {
                linear: offset.clone(),
                denied: None,
            }),
            (
                SegmentProfile::Flat32,
                SegmentSelection::Named(Segment::Ds | Segment::Es | Segment::Ss)
                | SegmentSelection::AddressDefault(_),
                Intent::Read | Intent::Write,
            ) => Ok(SegmentCheck {
                linear: offset.clone(),
                denied: None,
            }),
            _ => {
                let cache = self.cpu.read_segment(body, segment)?;
                Ok(SegmentCheck {
                    linear: cache.base.add(offset),
                    denied: Some(cache.access_denied(offset, bytes, intent)),
                })
            }
        }
    }
}

impl SegmentValues {
    fn bit(&self, mask: u16) -> Val<I1> {
        self.attributes.and(u32::from(mask)).ne(0)
    }

    fn access_denied(&self, offset: &Val<I32>, bytes: u32, intent: Intent) -> Val<I1> {
        let usable = self.bit(SegmentAttributes::USABLE);
        let code = self.bit(SegmentAttributes::CODE);
        let readable_or_writable = self.bit(SegmentAttributes::READ_WRITE);
        let down = self.bit(SegmentAttributes::EXPAND_DOWN);
        let permitted = match intent {
            Intent::Read => code.eq(0).or(&readable_or_writable),
            Intent::Write => code.eq(0).and(&readable_or_writable),
            Intent::Fetch => code.clone(),
        };
        let last = offset.add(bytes - 1);
        let no_wrap = last.unsigned().ge(offset);
        // Full-size expand-up segments permit wrapping offsets in this emulator.
        // Finite limits must cover every byte without offset arithmetic wrapping.
        let expand_up = self
            .limit
            .eq(u32::MAX)
            .or(no_wrap.and(self.limit.unsigned().ge(&last)));
        let upper = self
            .bit(SegmentAttributes::DEFAULT_BIG)
            .select(u32::MAX, 0xffffu32);
        let expand_down = self
            .limit
            .unsigned()
            .lt(offset)
            .and(&no_wrap)
            .and(upper.unsigned().ge(&last));
        usable
            .and(code.and(&down).eq(0))
            .and(permitted)
            .and(down.select(expand_down, expand_up))
            .eq(0)
    }
}
