use super::{CpuFeature, CpuModel};

#[test]
fn capabilities_are_enabled_by_default_and_can_be_toggled() {
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
        assert!(default.supports(feature));
        let disabled = default.with_feature(feature, false);
        assert!(!disabled.supports(feature));
        assert_eq!(disabled.with_feature(feature, true), default);
    }
}
