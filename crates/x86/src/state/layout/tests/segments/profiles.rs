use crate::{CpuState, ExecutionProfile, Segment, SegmentProfile, SegmentRegister, Segments};

fn compatible(segments: &Segments) -> bool {
    segments.is_compatible_with(SegmentProfile::Flat32)
}

#[test]
fn explicit_flat_initialization_satisfies_the_profile() {
    assert!(compatible(&Segments::flat32()));
    assert!(compatible(&CpuState::default().segments));
    assert!(!compatible(&Segments::default()));
}

#[test]
fn base_or_limit_changes_break_each_assumed_flat_segment() {
    for segment in [Segment::Cs, Segment::Ds, Segment::Es, Segment::Ss] {
        let mut segments = Segments::flat32();
        segments[segment].base = 0x1000;
        assert!(!compatible(&segments), "{segment:?} base");
        segments[segment].base = 0;
        segments[segment].limit = 0xffff;
        assert!(!compatible(&segments), "{segment:?} limit");
        segments[segment].limit = u32::MAX;
        assert!(compatible(&segments), "{segment:?} restored");
    }
}

#[test]
fn unusable_caches_break_the_profile_even_with_nonzero_selectors() {
    for segment in [Segment::Cs, Segment::Ds, Segment::Es, Segment::Ss] {
        let mut segments = Segments::flat32();
        segments[segment].selector = 0x23;
        segments[segment].usable = 0;
        assert!(!compatible(&segments), "{segment:?}");
    }
}

#[test]
fn code_defaults_and_stack_width_are_separate_requirements() {
    for segment in [Segment::Cs, Segment::Ss] {
        let mut segments = Segments::flat32();
        // Preserve the loaded type and access while clearing only D/B.
        segments[segment].big = 0;
        assert!(!compatible(&segments), "{segment:?}");
    }
    let mut segments = Segments::flat32();
    segments.ds.big = 0;
    segments.es.big = 0;
    assert!(
        compatible(&segments),
        "ordinary data D/B does not constrain this profile"
    );
}

#[test]
fn code_permissions_are_runtime_checks_in_the_segmented_profile() {
    let mut segments = Segments::flat32();
    segments.cs.readable = 0;
    assert!(!compatible(&segments));
    assert!(segments.is_compatible_with(SegmentProfile::Segmented32));
    for cache in [
        SegmentRegister::flat_data32(0x1b),
        SegmentRegister {
            big: 1,
            ..SegmentRegister::unusable(0x1b)
        },
    ] {
        segments.cs = cache;
        assert!(!compatible(&segments));
        assert!(segments.is_compatible_with(SegmentProfile::Segmented32));
    }
}

#[test]
fn assumed_data_segments_require_writable_expand_up_data() {
    for segment in [Segment::Ds, Segment::Es, Segment::Ss] {
        for change in [
            (|cache: &mut SegmentRegister| cache.writable = 0) as fn(&mut SegmentRegister),
            |cache| cache.expand_down = 1,
            |cache| cache.executable = 1,
            |cache| cache.readable = 0,
        ] {
            let mut segments = Segments::flat32();
            change(&mut segments[segment]);
            assert!(!compatible(&segments), "{segment:?}");
        }
    }
}

#[test]
fn ring_three_selectors_preserve_profile_assumptions() {
    let mut segments = Segments::flat32();
    for (segment, selector) in Segment::ALL
        .into_iter()
        .zip([0x23, 0x1b, 0x33, 0x43, 0x53, 0x63])
    {
        segments[segment].selector = selector;
    }
    assert!(compatible(&segments));
}

#[test]
fn fs_and_gs_changes_do_not_break_the_flat_profile() {
    let mut segments = Segments::flat32();
    segments.fs = SegmentRegister {
        base: 0x1234_5000,
        limit: 0xfff,
        ..SegmentRegister::flat_data32(0x53)
    };
    segments.gs = SegmentRegister::unusable(0x63);
    assert!(compatible(&segments));
}

#[test]
fn segmented_profiles_keep_access_checks_dynamic_with_ring_three_code_defaults() {
    for (profile, code_big) in [
        (SegmentProfile::Segmented16, 0),
        (SegmentProfile::Segmented32, 1),
    ] {
        for segment in Segment::ALL {
            let mut segments = Segments::flat32();
            segments[segment] = SegmentRegister {
                base: 0x8123_4000,
                limit: 0x4321,
                selector: 0x23,
                ..SegmentRegister::default()
            };
            segments.cs.big = code_big;
            assert!(segments.is_compatible_with(profile));
            segments.cs.big = 1 - code_big;
            assert!(!segments.is_compatible_with(profile));
        }
    }
}

#[test]
fn profile_constants_require_canonical_boolean_bytes() {
    for segment in [Segment::Cs, Segment::Ds, Segment::Es, Segment::Ss] {
        let mut segments = Segments::flat32();
        segments[segment].usable = 2;
        assert!(!compatible(&segments));
    }
    let mut segments = Segments::flat32();
    segments.cs.big = 2;
    assert!(!segments.is_compatible_with(SegmentProfile::Segmented32));
}

#[test]
fn real_mode_requires_canonical_caches_but_accepts_every_segment_value() {
    let mut segments = Segments::real_mode();
    assert!(segments.is_compatible_with(ExecutionProfile::Real16));
    for segment in Segment::ALL {
        for selector in [0, 3, 4, 0xffff] {
            let cache = SegmentRegister::real_mode(segment, selector);
            assert_eq!(cache.base, u32::from(selector) * 16);
            assert_eq!(cache.limit, 65535);
            assert_eq!(cache.big, 0);
            assert_eq!(cache.executable, u8::from(segment == Segment::Cs));
            assert_eq!(cache.writable, u8::from(segment != Segment::Cs));
            segments[segment] = cache;
            assert!(segments.is_compatible_with(ExecutionProfile::Real16));
            for bad in [
                SegmentRegister {
                    base: cache.base + 1,
                    ..cache
                },
                SegmentRegister {
                    limit: u32::MAX,
                    ..cache
                },
                SegmentRegister { big: 1, ..cache },
                SegmentRegister::unusable(selector),
            ] {
                segments[segment] = bad;
                assert!(!segments.is_compatible_with(ExecutionProfile::Real16));
            }
            segments[segment] = cache;
        }
    }
    assert!(!Segments::flat32().is_compatible_with(ExecutionProfile::Real16));
}
