use super::data;
use crate::{
    CpuState, DescriptorTables, Exception::*, PrivilegeLevel::*, Segment, SegmentAttributes,
    SegmentDefaultSize::*, SegmentDescriptor, SegmentDescriptorKind::*, SegmentProfile,
    StoredSegment,
};

#[test]
fn data_register_loads_check_type_and_dpl_even_with_privileged_rpl() {
    let mut tables = DescriptorTables::default();
    for (kind, lower_dpl_allowed) in [
        (
            Data {
                writable: false,
                expand_down: false,
            },
            false,
        ),
        (
            Data {
                writable: true,
                expand_down: true,
            },
            false,
        ),
        (
            Code {
                readable: true,
                conforming: false,
            },
            false,
        ),
        (
            Code {
                readable: true,
                conforming: true,
            },
            true,
        ),
    ] {
        for dpl in [Ring0, Ring1, Ring2, Ring3] {
            tables.insert(
                0x23,
                SegmentDescriptor {
                    kind,
                    dpl,
                    ..data()
                },
            );
            for selector in [0x20, 0x21, 0x22, 0x23] {
                for segment in [Segment::Ds, Segment::Es, Segment::Fs, Segment::Gs] {
                    let result = tables.resolve_user_segment(segment, selector);
                    if dpl == Ring3 || lower_dpl_allowed {
                        assert_eq!(result.unwrap().selector, selector);
                    } else {
                        assert_eq!(result, Err(GeneralProtection { error_code: 0x20 }));
                    }
                }
            }
        }
    }
    for conforming in [false, true] {
        tables.insert(
            0x23,
            SegmentDescriptor {
                kind: Code {
                    readable: false,
                    conforming,
                },
                ..data()
            },
        );
        for segment in [Segment::Ds, Segment::Es, Segment::Fs, Segment::Gs] {
            assert_eq!(
                tables.resolve_user_segment(segment, 0x23),
                Err(GeneralProtection { error_code: 0x20 })
            );
        }
    }
}

#[test]
fn stack_loads_require_writable_ring_three_data_and_ring_three_rpl() {
    let mut tables = DescriptorTables::default();
    for dpl in [Ring0, Ring1, Ring2, Ring3] {
        tables.insert(0x27, SegmentDescriptor { dpl, ..data() });
        for selector in [0x24, 0x25, 0x26, 0x27] {
            let result = tables.resolve_user_segment(Segment::Ss, selector);
            if dpl == Ring3 && selector == 0x27 {
                assert!(result.is_ok());
            } else {
                assert_eq!(result, Err(GeneralProtection { error_code: 0x24 }));
            }
        }
    }
    for kind in [
        Data {
            writable: false,
            expand_down: false,
        },
        Code {
            readable: true,
            conforming: false,
        },
        Code {
            readable: true,
            conforming: true,
        },
    ] {
        tables.insert(0x27, SegmentDescriptor { kind, ..data() });
        assert_eq!(
            tables.resolve_user_segment(Segment::Ss, 0x27),
            Err(GeneralProtection { error_code: 0x24 })
        );
    }
    for (default_size, bits) in [(Bits16, 0x0d), (Bits32, 0x1d)] {
        tables.insert(
            0x27,
            SegmentDescriptor {
                kind: Data {
                    writable: true,
                    expand_down: true,
                },
                default_size,
                ..data()
            },
        );
        assert_eq!(
            tables
                .resolve_user_segment(Segment::Ss, 0x27)
                .unwrap()
                .attributes
                .bits(),
            bits
        );
    }
}

#[test]
fn direct_code_resolution_uses_call_jump_rules_and_normalizes_cs_rpl() {
    let mut tables = DescriptorTables::default();
    for conforming in [false, true] {
        for dpl in [Ring0, Ring1, Ring2, Ring3] {
            tables.insert(
                0x23,
                SegmentDescriptor {
                    kind: Code {
                        readable: false,
                        conforming,
                    },
                    dpl,
                    default_size: Bits16,
                    ..data()
                },
            );
            for selector in [0x20, 0x21, 0x22, 0x23] {
                let result = tables.resolve_user_segment(Segment::Cs, selector);
                if conforming || dpl == Ring3 {
                    let expected = StoredSegment {
                        base: 0x1234_5000,
                        limit: 0xffff,
                        selector: 0x23,
                        attributes: SegmentAttributes::from_bits(3),
                    };
                    assert_eq!(result, Ok(expected));
                    let mut cpu = CpuState::default();
                    cpu.segments.cs = expected;
                    assert!(SegmentProfile::Segmented16.is_compatible_with(&cpu.segments));
                    assert!(!SegmentProfile::Segmented32.is_compatible_with(&cpu.segments));
                } else {
                    assert_eq!(result, Err(GeneralProtection { error_code: 0x20 }));
                }
            }
        }
    }
    tables.insert(0x23, data());
    assert_eq!(
        tables.resolve_user_segment(Segment::Cs, 0x23),
        Err(GeneralProtection { error_code: 0x20 })
    );
}

#[test]
fn load_faults_preserve_table_bits_and_check_access_before_presence() {
    let mut tables = DescriptorTables::default();
    for selector in [0x23, 0x27, 0xfffb, 0xffff] {
        let error_code = match selector {
            0x23 => 0x20,
            0x27 => 0x24,
            0xfffb => 0xfff8,
            _ => 0xfffc,
        };
        assert_eq!(
            tables.resolve_user_segment(Segment::Ds, selector),
            Err(GeneralProtection { error_code })
        );
    }
    tables.insert(
        0x27,
        SegmentDescriptor {
            present: false,
            ..data()
        },
    );
    for segment in [Segment::Ds, Segment::Es, Segment::Fs, Segment::Gs] {
        assert_eq!(
            tables.resolve_user_segment(segment, 0x27),
            Err(SegmentNotPresent { error_code: 0x24 })
        );
    }
    assert_eq!(
        tables.resolve_user_segment(Segment::Ss, 0x27),
        Err(StackFault { error_code: 0x24 })
    );
    assert_eq!(
        tables.resolve_user_segment(Segment::Ss, 0x24),
        Err(GeneralProtection { error_code: 0x24 })
    );
    assert_eq!(
        tables.resolve_user_segment(Segment::Cs, 0x27),
        Err(GeneralProtection { error_code: 0x24 })
    );
    tables.insert(
        0x27,
        SegmentDescriptor {
            dpl: Ring0,
            present: false,
            ..data()
        },
    );
    assert_eq!(
        tables.resolve_user_segment(Segment::Ds, 0x27),
        Err(GeneralProtection { error_code: 0x24 })
    );
    tables.insert(
        0x27,
        SegmentDescriptor {
            kind: Code {
                readable: true,
                conforming: false,
            },
            present: false,
            ..data()
        },
    );
    assert_eq!(
        tables.resolve_user_segment(Segment::Cs, 0x27),
        Err(SegmentNotPresent { error_code: 0x24 })
    );
    assert_eq!(
        tables.resolve_user_segment(Segment::Ss, 0x27),
        Err(GeneralProtection { error_code: 0x24 })
    );
}
