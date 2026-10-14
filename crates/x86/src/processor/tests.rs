use super::{CpuFeature, CpuModel, UnsupportedCpuFeature};

#[test]
fn complete_capabilities_are_enabled_by_default_and_can_be_toggled() {
    let default = CpuModel::default();
    for feature in [
        CpuFeature::Cx8,
        CpuFeature::Cmov,
        CpuFeature::Tsc,
        CpuFeature::Fpu,
        CpuFeature::Mmx,
        CpuFeature::Fxsr,
        CpuFeature::Sse,
        CpuFeature::Sse2,
        CpuFeature::Clflush,
    ] {
        assert!(feature.is_implemented());
        assert!(feature.is_complete());
        assert!(default.supports(feature));
        let disabled = default.with_feature(feature, false).unwrap();
        assert!(!disabled.supports(feature));
        assert_eq!(disabled.with_feature(feature, true).unwrap(), default);
    }
}

#[test]
fn partial_sse3_is_enabled_without_claiming_complete_support() {
    let default = CpuModel::default();
    assert!(CpuFeature::Sse3.is_implemented());
    assert!(!CpuFeature::Sse3.is_complete());
    assert!(default.supports(CpuFeature::Sse3));
    let disabled = default.with_feature(CpuFeature::Sse3, false).unwrap();
    assert!(!disabled.supports(CpuFeature::Sse3));
    assert_eq!(
        disabled.with_feature(CpuFeature::Sse3, true).unwrap(),
        default
    );
}

#[test]
fn unavailable_capabilities_cannot_be_enabled_or_change_the_model() {
    let default = CpuModel::default();
    for feature in [CpuFeature::Sep, CpuFeature::Monitor] {
        assert!(!feature.is_implemented());
        assert!(!feature.is_complete());
        assert!(!default.supports(feature));
        assert_eq!(
            default.with_feature(feature, true),
            Err(UnsupportedCpuFeature { feature })
        );
        assert_eq!(default.with_feature(feature, false).unwrap(), default);
    }
}
