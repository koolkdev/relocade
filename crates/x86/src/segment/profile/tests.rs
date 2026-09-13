use crate::{CpuState, Segment, SegmentAttributes, SegmentProfile, Segments, StoredSegment};

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
        segments[segment].attributes = SegmentAttributes::unusable();
        assert!(!compatible(&segments), "{segment:?}");
    }
}

#[test]
fn code_defaults_and_stack_width_are_separate_requirements() {
    for segment in [Segment::Cs, Segment::Ss] {
        let mut segments = Segments::flat32();
        // Preserve the loaded type and access while clearing only D/B.
        segments[segment].attributes =
            SegmentAttributes::from_bits(if segment == Segment::Cs { 0x07 } else { 0x05 });
        assert!(!compatible(&segments), "{segment:?}");
    }
    let mut segments = Segments::flat32();
    segments.ds.attributes = SegmentAttributes::from_bits(0x05);
    segments.es.attributes = SegmentAttributes::from_bits(0x05);
    assert!(
        compatible(&segments),
        "ordinary data D/B does not constrain this profile"
    );
}

#[test]
fn code_permissions_are_runtime_checks_in_the_segmented_profile() {
    let mut segments = Segments::flat32();
    segments.cs.attributes = SegmentAttributes::from_bits(0x13);
    assert!(!compatible(&segments));
    assert!(SegmentProfile::Segmented32.is_compatible_with(&segments));
    for bits in [0x15, 0x1b, 0xffff] {
        segments.cs.attributes = SegmentAttributes::from_bits(bits);
        assert!(!compatible(&segments), "CS attributes {bits:04x}");
        assert!(SegmentProfile::Segmented32.is_compatible_with(&segments));
    }
}

#[test]
fn assumed_data_segments_require_writable_expand_up_data() {
    for segment in [Segment::Ds, Segment::Es, Segment::Ss] {
        for bits in [0x11, 0x19, 0x1d, 0x13, 0x17] {
            let mut segments = Segments::flat32();
            segments[segment].attributes = SegmentAttributes::from_bits(bits);
            assert!(!compatible(&segments), "{segment:?} attributes {bits:04x}");
        }
    }
}

#[test]
fn selectors_and_reserved_attribute_bits_do_not_change_assumptions() {
    let mut segments = Segments::flat32();
    for (segment, selector) in Segment::ALL
        .into_iter()
        .zip([0x23, 0x1b, 0x33, 0x43, 0x53, 0x63])
    {
        segments[segment].selector = selector;
        segments[segment].attributes =
            SegmentAttributes::from_bits(segments[segment].attributes.bits() | 0xffe0);
    }
    assert!(compatible(&segments));
}

#[test]
fn fs_and_gs_changes_do_not_break_the_flat_profile() {
    let mut segments = Segments::flat32();
    segments.fs = StoredSegment {
        base: 0x1234_5000,
        limit: 0xfff,
        ..StoredSegment::flat_data32(0x53)
    };
    segments.gs = StoredSegment::unusable(0x63);
    assert!(compatible(&segments));
}

#[test]
fn runtime_checks_allow_nonflat_or_unusable_caches_with_supported_defaults() {
    for segment in Segment::ALL {
        for attributes in [0x10, 0x11, 0x13, 0x17, 0x1d] {
            let mut segments = Segments::flat32();
            segments[segment] = StoredSegment {
                base: 0x8123_4000,
                limit: 0x4321,
                selector: 0x23,
                attributes: SegmentAttributes::from_bits(attributes),
            };
            assert!(
                SegmentProfile::Segmented32.is_compatible_with(&segments),
                "{segment:?}, {attributes:02x}"
            );
        }
    }
}

#[test]
fn segmented_profiles_assume_only_their_code_default_size() {
    for (profile, code_bits) in [
        (SegmentProfile::Segmented16, 0x07),
        (SegmentProfile::Segmented32, 0x17),
    ] {
        for stack_bits in [0, 0x05, 0x15, 0xffff] {
            let mut segments = Segments::flat32();
            segments.cs.attributes = SegmentAttributes::from_bits(code_bits);
            segments.ss.attributes = SegmentAttributes::from_bits(stack_bits);
            assert!(profile.is_compatible_with(&segments));
            segments.cs.attributes = SegmentAttributes::from_bits(code_bits ^ 0x10);
            assert!(!profile.is_compatible_with(&segments));
        }
    }
}
