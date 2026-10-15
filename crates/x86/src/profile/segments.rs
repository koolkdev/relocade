//! Segment assumptions shared by host admission and generated dispatch.

use super::ExecutionProfile;
use crate::{
    state::Cpu, Segment, SegmentAttributes, SegmentDefaultSize, SegmentProfile, Segments,
    StoredSegment,
};
use wasm86_compiler::{BlockBuilder, BuildError, Val, I1, I32};

#[derive(Clone, Copy)]
enum RangeAssumption {
    Any,
    Flat,
    Real,
}

struct SegmentAssumptions {
    range: RangeAssumption,
    attribute_mask: u16,
    attributes: u16,
}

impl ExecutionProfile {
    fn assumptions(self, segment: Segment) -> Option<SegmentAssumptions> {
        let kind_mask = SegmentAttributes::USABLE
            | SegmentAttributes::CODE
            | SegmentAttributes::READ_WRITE
            | SegmentAttributes::EXPAND_DOWN;
        let size_mask = SegmentAttributes::DEFAULT_BIG;
        let (range, attribute_mask, attributes) = match self {
            Self::Real16 => (
                RangeAssumption::Real,
                u16::MAX,
                StoredSegment::real_mode(segment, 0).attributes.bits(),
            ),
            Self::Protected(SegmentProfile::Flat32) => match segment {
                Segment::Cs => (
                    RangeAssumption::Flat,
                    kind_mask | size_mask,
                    StoredSegment::flat_code32(0).attributes.bits(),
                ),
                Segment::Ss => (
                    RangeAssumption::Flat,
                    kind_mask | size_mask,
                    StoredSegment::flat_data32(0).attributes.bits(),
                ),
                Segment::Ds | Segment::Es => (
                    RangeAssumption::Flat,
                    kind_mask,
                    StoredSegment::flat_data32(0).attributes.bits() & kind_mask,
                ),
                Segment::Fs | Segment::Gs => return None,
            },
            Self::Protected(profile) => {
                if segment != Segment::Cs {
                    return None;
                }
                (
                    RangeAssumption::Any,
                    size_mask,
                    if profile.code_default_size() == SegmentDefaultSize::Bits32 {
                        size_mask
                    } else {
                        0
                    },
                )
            }
        };
        Some(SegmentAssumptions {
            range,
            attribute_mask,
            attributes,
        })
    }

    pub(super) fn matches_segments(self, segments: &Segments) -> bool {
        Segment::ALL.into_iter().all(|segment| {
            self.assumptions(segment).is_none_or(|required| {
                let cache = segments[segment];
                let range_matches = match required.range {
                    RangeAssumption::Any => true,
                    RangeAssumption::Flat => cache.base == 0 && cache.limit == u32::MAX,
                    RangeAssumption::Real => {
                        cache.base == u32::from(cache.selector) << 4 && cache.limit == 0xffff
                    }
                };
                range_matches
                    && cache.attributes.bits() & required.attribute_mask == required.attributes
            })
        })
    }

    pub(crate) fn check_segments(
        self,
        body: &mut BlockBuilder<'_>,
        cpu: &Cpu,
    ) -> Result<Val<I1>, BuildError> {
        let mut compatible = Val::<I1>::from(true);
        for segment in Segment::ALL {
            let Some(required) = self.assumptions(segment) else {
                continue;
            };
            let cache = cpu.read_segment(body, &segment.into())?;
            let attributes = cache
                .attributes
                .and(u32::from(required.attribute_mask))
                .eq(u32::from(required.attributes));
            let range = match required.range {
                RangeAssumption::Any => true.into(),
                RangeAssumption::Flat => cache.base.eq(0).and(cache.limit.eq(u32::MAX)),
                RangeAssumption::Real => cache
                    .base
                    .eq(cache.selector.unsigned().extend::<I32>().shl(4))
                    .and(cache.limit.eq(0xffffu32)),
            };
            compatible = compatible.and(attributes).and(range);
        }
        Ok(compatible)
    }
}
