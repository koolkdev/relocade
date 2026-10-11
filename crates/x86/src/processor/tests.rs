use super::{CpuFeature, CpuModel, UnsupportedCpuFeature};

#[test]
fn complete_capabilities_can_be_disabled_and_enabled_again() {
    let default = CpuModel::default();
    for feature in [CpuFeature::Cx8, CpuFeature::Cmov] {
        assert!(feature.is_implemented());
        assert!(default.supports(feature));
        let disabled = default.with_feature(feature, false).unwrap();
        assert!(!disabled.supports(feature));
        assert_eq!(disabled.with_feature(feature, true).unwrap(), default);
    }
}

#[test]
fn unavailable_capabilities_cannot_be_enabled_or_change_the_model() {
    let default = CpuModel::default();
    for feature in [
        CpuFeature::Tsc,
        CpuFeature::Fpu,
        CpuFeature::Mmx,
        CpuFeature::Fxsr,
        CpuFeature::Sse,
        CpuFeature::Sse2,
        CpuFeature::Sse3,
        CpuFeature::Clflush,
        CpuFeature::Sep,
        CpuFeature::Monitor,
    ] {
        assert!(!feature.is_implemented());
        assert!(!default.supports(feature));
        assert_eq!(
            default.with_feature(feature, true),
            Err(UnsupportedCpuFeature { feature })
        );
        assert_eq!(default.with_feature(feature, false).unwrap(), default);
    }
}
