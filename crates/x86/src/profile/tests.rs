use crate::{CpuState, ExecutionProfile};

#[test]
fn profile_admission_requires_supported_mode_and_control_state() {
    use crate::SegmentProfile;
    for (profile, cpu) in [
        (ExecutionProfile::Real16, CpuState::real_mode()),
        (SegmentProfile::Flat32.into(), CpuState::default()),
    ] {
        assert!(cpu.is_compatible_with(profile));
        for change in [
            (|cpu: &mut CpuState| cpu.cr0 ^= 1) as fn(&mut CpuState),
            |cpu| cpu.cr0 |= 1 << 2,     // EM
            |cpu| cpu.cr0 |= 1 << 3,     // TS
            |cpu| cpu.cr0 &= !(1 << 5),  // NE
            |cpu| cpu.cr0 |= 1 << 31,    // PG
            |cpu| cpu.cr4 |= 1 << 2,     // TSD
            |cpu| cpu.cr4 &= !(1 << 9),  // OSFXSR
            |cpu| cpu.cr4 &= !(1 << 10), // OSXMMEXCPT
            |cpu| cpu.flags.bytes.vm = 1,
        ] {
            let mut changed = cpu;
            change(&mut changed);
            assert!(!changed.is_compatible_with(profile), "{changed:?}");
        }
    }
    for cpl in 0..3 {
        let mut cpu = CpuState::default();
        cpu.segments.cs.selector = 0x18 | cpl;
        assert!(!cpu.is_compatible_with(SegmentProfile::Flat32));
        assert!(!cpu.is_compatible_with(SegmentProfile::Segmented32));
    }
}

#[test]
fn segmented_admission_accepts_independent_stack_widths() {
    use crate::{SegmentDefaultSize::*, SegmentProfile};
    for (code, profile, other) in [
        (
            Bits16,
            SegmentProfile::Segmented16,
            SegmentProfile::Segmented32,
        ),
        (
            Bits32,
            SegmentProfile::Segmented32,
            SegmentProfile::Segmented16,
        ),
    ] {
        for stack in [Bits16, Bits32] {
            let cpu = CpuState::protected_mode(code, stack);
            assert!(cpu.is_compatible_with(profile));
            assert!(!cpu.is_compatible_with(other));
            assert_eq!(
                cpu.is_compatible_with(SegmentProfile::Flat32),
                code == Bits32 && stack == Bits32
            );
        }
    }
}

#[test]
fn entry_compatibility_requires_the_complete_target_and_current_segment_assumptions() {
    use crate::{Compiler, CpuFeature, CpuModel, SegmentProfile};
    let target = Compiler::new(SegmentProfile::Flat32).target();
    let flat = CpuState::default();
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
    assert!(!target.is_compatible_with(target, &CpuState::real_mode()));
    let real = Compiler::new(ExecutionProfile::Real16).target();
    assert!(real.is_compatible_with(real, &CpuState::real_mode()));
    assert!(!real.is_compatible_with(target, &CpuState::real_mode()));
}
