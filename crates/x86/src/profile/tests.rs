use crate::{ExecutionProfile, Segment, SegmentAttributes, Segments, StoredSegment};

#[test]
fn entry_compatibility_requires_the_complete_target_and_current_segment_assumptions() {
    use crate::{Compiler, CpuFeature, CpuModel, SegmentProfile};
    let target = Compiler::new(SegmentProfile::Flat32).target();
    let flat = Segments::flat32();
    assert!(target.is_compatible_with(target, &flat));
    let restricted = Compiler::new(SegmentProfile::Flat32)
        .with_cpu_model(
            CpuModel::default()
                .with_feature(CpuFeature::Cmov, false)
                .unwrap(),
        )
        .target();
    assert!(!target.is_compatible_with(restricted, &flat));
    assert!(!restricted.is_compatible_with(target, &flat));
    let other_profile = Compiler::new(SegmentProfile::Segmented32).target();
    assert!(!target.is_compatible_with(other_profile, &flat));
    assert!(!target.is_compatible_with(target, &Segments::real_mode()));
    let real = Compiler::new(ExecutionProfile::Real16).target();
    assert!(real.is_compatible_with(real, &Segments::real_mode()));
    assert!(!real.is_compatible_with(target, &Segments::real_mode()));
}

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
