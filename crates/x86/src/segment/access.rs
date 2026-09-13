//! Segment permissions and complete offset spans are checked before paging.

use wasm86_compiler::{BuildError, FunctionBuilder, MemoryInt, Val, I1, I16, I32};

use crate::{exception::Exception, memory::Intent, state::Cpu};

use super::{Segment, SegmentAttributes, SegmentProfile, SegmentSelection};

/// Values read from one loaded cache. The selector does not determine access.
pub(crate) struct SegmentValues {
    pub(crate) base: Val<I32>,
    pub(crate) limit: Val<I32>,
    pub(crate) attributes: Val<I16>,
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

    /// The entry's profile must remain compatible for its entire invocation.
    /// Flat ordinary segments need no cache reads or per-access segment guards.
    pub(crate) fn translate<T: MemoryInt>(
        &self,
        body: &mut FunctionBuilder<'_>,
        segment: &SegmentSelection,
        offset: &Val<I32>,
        intent: Intent,
        on_fault: impl Fn(FunctionBuilder<'_>, Exception) -> Result<(), BuildError>,
    ) -> Result<Val<I32>, BuildError> {
        if self.profile == SegmentProfile::Flat32
            && matches!(
                segment.known(),
                Some(Segment::Ds | Segment::Es | Segment::Ss)
            )
            && !matches!(intent, Intent::Fetch)
        {
            return Ok(offset.clone());
        }
        let cache = self.cpu.read_segment(body, segment)?;
        let denied = cache.access_denied::<T>(offset, intent);
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
        Ok(cache.base.add(offset))
    }
}

impl SegmentValues {
    fn bit(&self, mask: u16) -> Val<I1> {
        self.attributes.and(u32::from(mask)).ne(0)
    }

    fn access_denied<T: MemoryInt>(&self, offset: &Val<I32>, intent: Intent) -> Val<I1> {
        let usable = self.bit(SegmentAttributes::USABLE);
        let code = self.bit(SegmentAttributes::CODE);
        let readable_or_writable = self.bit(SegmentAttributes::READ_WRITE);
        let down = self.bit(SegmentAttributes::EXPAND_DOWN);
        let permitted = match intent {
            Intent::Read => code.eq(0).or(&readable_or_writable),
            Intent::Write => code.eq(0).and(&readable_or_writable),
            Intent::Fetch => code.clone(),
        };
        let last = offset.add(T::BYTES - 1);
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
