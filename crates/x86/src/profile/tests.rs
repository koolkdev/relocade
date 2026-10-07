use crate::{ExecutionProfile, Segment, SegmentAttributes, Segments, StoredSegment};

#[test]
fn real_mode_requires_canonical_caches_but_accepts_every_segment_value() {
    let mut segments = Segments::real_mode();
    assert!(ExecutionProfile::Real16.is_compatible_with(&segments));
    for segment in Segment::ALL {
        for selector in [0, 3, 4, 0xffff] {
            let cache = StoredSegment::real_mode(segment, selector);
            assert_eq!(cache.base, u32::from(selector) * 16);
            assert_eq!(cache.limit, 65535);
            assert_eq!(
                cache.attributes.bits(),
                if segment == Segment::Cs { 7 } else { 5 }
            );
            segments[segment] = cache;
            assert!(ExecutionProfile::Real16.is_compatible_with(&segments));
            for bad in [
                StoredSegment {
                    base: cache.base + 1,
                    ..cache
                },
                StoredSegment {
                    limit: u32::MAX,
                    ..cache
                },
                StoredSegment {
                    attributes: SegmentAttributes::from_bits(cache.attributes.bits() | 16),
                    ..cache
                },
                StoredSegment::unusable(selector),
            ] {
                segments[segment] = bad;
                assert!(!ExecutionProfile::Real16.is_compatible_with(&segments));
            }
            segments[segment] = cache;
        }
    }
    assert!(!ExecutionProfile::Real16.is_compatible_with(&Segments::flat32()));
}
