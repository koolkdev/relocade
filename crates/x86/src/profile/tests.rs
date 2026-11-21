use crate::support::config::{FLAT32, REAL16, SEGMENTED16, SEGMENTED32};
use crate::CpuState;

#[test]
fn admission_requires_supported_mode_and_control_state() {
    for (config, cpu) in [
        (REAL16, CpuState::real_mode()),
        (FLAT32, CpuState::default()),
    ] {
        assert!(cpu.is_compatible_with(config.entry));
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
            assert!(!changed.is_compatible_with(config.entry), "{changed:?}");
        }
    }
    for cpl in 0..3 {
        let mut cpu = CpuState::default();
        cpu.segments.cs.selector = 0x18 | cpl;
        assert!(!cpu.is_compatible_with(FLAT32.entry));
        assert!(!cpu.is_compatible_with(SEGMENTED32.entry));
    }
}

#[test]
fn segmented_admission_accepts_independent_stack_widths() {
    use crate::SegmentDefaultSize::*;
    for (code, config, other) in [
        (Bits16, SEGMENTED16, SEGMENTED32),
        (Bits32, SEGMENTED32, SEGMENTED16),
    ] {
        for stack in [Bits16, Bits32] {
            let cpu = CpuState::protected_mode(code, stack);
            assert!(cpu.is_compatible_with(config.entry));
            assert!(!cpu.is_compatible_with(other.entry));
            assert_eq!(
                cpu.is_compatible_with(FLAT32.entry),
                code == Bits32 && stack == Bits32
            );
        }
    }
}

#[test]
fn code_width_and_segment_guarantees_are_admitted_independently() {
    use crate::{EntryAssumptions, SegmentDefaultSize::*};
    let live_width = EntryAssumptions {
        code_default_size: None,
        ..EntryAssumptions::FLAT32
    };
    for code in [Bits16, Bits32] {
        let mut cpu = CpuState::protected_mode(code, Bits32);
        assert!(cpu.is_compatible_with(live_width));
        cpu.segments.ds.base = 100;
        assert!(!cpu.is_compatible_with(live_width));
    }
    let flat16 = EntryAssumptions {
        code_default_size: Some(Bits16),
        ..live_width
    };
    assert!(CpuState::protected_mode(Bits16, Bits32).is_compatible_with(flat16));
    assert!(!CpuState::protected_mode(Bits32, Bits32).is_compatible_with(flat16));
    let real32 = EntryAssumptions {
        code_default_size: Some(Bits32),
        ..EntryAssumptions::REAL_MODE
    };
    assert!(!CpuState::real_mode().is_compatible_with(real32));
}

#[test]
fn entry_compatibility_requires_matching_codegen_inputs_and_current_segment_assumptions() {
    use crate::{Compiler, CompilerConfig, CpuFeature, CpuModel, MemoryBackend};
    let config = FLAT32;
    let flat = CpuState::default();
    assert!(config.is_compatible_with(config, &flat));
    let physical = CompilerConfig {
        memory_backend: MemoryBackend::PhysicalBus,
        ..config
    };
    assert!(!physical.is_compatible_with(config, &flat));
    let restricted = Compiler::from_config(FLAT32)
        .with_cpu_model(CpuModel::default().with_feature(CpuFeature::Cmov, false))
        .config();
    assert!(!config.is_compatible_with(restricted, &flat));
    assert!(!restricted.is_compatible_with(config, &flat));
    let unlimited = Compiler::from_config(FLAT32)
        .with_instruction_budgets(false)
        .config();
    assert!(!config.is_compatible_with(unlimited, &flat));
    assert!(!unlimited.is_compatible_with(config, &flat));
    let segmented = SEGMENTED32;
    assert!(!config.is_compatible_with(segmented, &flat));
    assert!(!config.is_compatible_with(config, &CpuState::real_mode()));
    let real = REAL16;
    assert!(real.is_compatible_with(real, &CpuState::real_mode()));
    assert!(!real.is_compatible_with(config, &CpuState::real_mode()));
}
