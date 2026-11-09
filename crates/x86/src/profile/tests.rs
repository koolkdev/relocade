use crate::{ExecutionProfile, Segments};

#[test]
fn entry_compatibility_requires_the_complete_target_and_current_segment_assumptions() {
    use crate::{Compiler, CpuFeature, CpuModel, SegmentProfile};
    let target = Compiler::new(SegmentProfile::Flat32).target();
    let flat = Segments::flat32();
    assert!(target.is_compatible_with(target, &flat));
    let restricted = Compiler::new(SegmentProfile::Flat32)
        .with_cpu_model(CpuModel::default().with_feature(CpuFeature::Cmov, false))
        .target();
    assert!(!target.is_compatible_with(restricted, &flat));
    assert!(!restricted.is_compatible_with(target, &flat));
    let unlimited = Compiler::new(SegmentProfile::Flat32)
        .with_instruction_budgets(false)
        .target();
    assert!(!target.is_compatible_with(unlimited, &flat));
    assert!(!unlimited.is_compatible_with(target, &flat));
    let other_profile = Compiler::new(SegmentProfile::Segmented32).target();
    assert!(!target.is_compatible_with(other_profile, &flat));
    assert!(!target.is_compatible_with(target, &Segments::real_mode()));
    let real = Compiler::new(ExecutionProfile::Real16).target();
    assert!(real.is_compatible_with(real, &Segments::real_mode()));
    assert!(!real.is_compatible_with(target, &Segments::real_mode()));
}
