// Compiled by both the build script and the x86 unit tests so fixture lookup uses
// the same targets as generation, without exposing private x86 test helpers.
use super::x86::{
    Compiler, CpuFeature, CpuModel, ExecutionProfile, ExecutionTarget, SegmentProfile,
};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum InterpreterEntry {
    Step,
    Run,
}

pub fn targets() -> Vec<(ExecutionTarget, InterpreterEntry)> {
    use CpuFeature::*;
    use InterpreterEntry::*;
    let features = [Cx8, Cmov, Tsc, Fpu, Mmx, Fxsr, Sse, Sse2, Clflush];
    let mut targets = Vec::new();
    for profile in [
        SegmentProfile::Flat32.into(),
        SegmentProfile::Segmented32.into(),
        SegmentProfile::Segmented16.into(),
        ExecutionProfile::Real16,
    ] {
        for entry in [Step, Run] {
            targets.push((Compiler::new(profile).target(), entry));
        }
    }
    for feature in features {
        targets.push((
            Compiler::new(SegmentProfile::Flat32)
                .with_cpu_model(CpuModel::default().with_feature(feature, false))
                .target(),
            Step,
        ));
    }
    for feature in [Cx8, Cmov, Tsc] {
        targets.push((
            Compiler::new(ExecutionProfile::Real16)
                .with_cpu_model(CpuModel::default().with_feature(feature, false))
                .target(),
            Step,
        ));
    }
    let disabled = features
        .into_iter()
        .fold(CpuModel::default(), |model, feature| {
            model.with_feature(feature, false)
        });
    targets.push((
        Compiler::new(SegmentProfile::Flat32)
            .with_cpu_model(disabled)
            .target(),
        Step,
    ));
    targets.push((
        Compiler::new(SegmentProfile::Flat32)
            .with_cpu_model(CpuModel::default().with_feature(Cmov, false))
            .target(),
        Run,
    ));
    targets
}
