use crate::{CpuState, Segment, SegmentProfile, SegmentRegister, Segments};

fn compatible(segments: &Segments) -> bool {
    SegmentProfile::Flat32.is_compatible_with(segments)
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
    assert!(SegmentProfile::Segmented32.is_compatible_with(&segments));
    for cache in [
        SegmentRegister::flat_data32(0),
        SegmentRegister {
            big: 1,
            ..SegmentRegister::unusable(0)
        },
    ] {
        segments.cs = cache;
        assert!(!compatible(&segments));
        assert!(SegmentProfile::Segmented32.is_compatible_with(&segments));
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
fn selectors_do_not_change_assumptions() {
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
fn segmented_profiles_assume_only_their_code_default_size() {
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
            assert!(profile.is_compatible_with(&segments));
            segments.cs.big = 1 - code_big;
            assert!(!profile.is_compatible_with(&segments));
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
    assert!(!SegmentProfile::Segmented32.is_compatible_with(&segments));
}
