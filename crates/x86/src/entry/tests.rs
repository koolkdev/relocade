use crate::{
    support::config::{FLAT32, REAL16, SEGMENTED32},
    CompilerConfig, CpuState, EntryAssumptions, MemoryBackend,
};

#[test]
fn entry_assumptions_require_supported_mode_and_control_state() {
    for (entry, cpu) in [
        (EntryAssumptions::REAL_MODE, CpuState::real_mode()),
        (EntryAssumptions::FLAT32, CpuState::default()),
    ] {
        assert!(cpu.is_compatible_with(entry));
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
            assert!(!changed.is_compatible_with(entry), "{changed:?}");
        }
    }
    for cpl in 0..3 {
        let mut cpu = CpuState::default();
        cpu.segments.cs.selector = 0x18 | cpl;
        assert!(!cpu.is_compatible_with(EntryAssumptions::FLAT32));
        assert!(!cpu.is_compatible_with(EntryAssumptions::SEGMENTED32));
    }
}

#[test]
fn segmented_admission_accepts_independent_stack_widths() {
    use crate::SegmentDefaultSize::*;
    for (code, entry, other) in [
        (
            Bits16,
            EntryAssumptions::SEGMENTED16,
            EntryAssumptions::SEGMENTED32,
        ),
        (
            Bits32,
            EntryAssumptions::SEGMENTED32,
            EntryAssumptions::SEGMENTED16,
        ),
    ] {
        for stack in [Bits16, Bits32] {
            let cpu = CpuState::protected_mode(code, stack);
            assert!(cpu.is_compatible_with(entry));
            assert!(!cpu.is_compatible_with(other));
            assert_eq!(
                cpu.is_compatible_with(EntryAssumptions::FLAT32),
                code == Bits32 && stack == Bits32
            );
        }
    }
}

#[test]
fn compiled_entries_require_matching_configuration_and_cpu_state() {
    use crate::{CpuFeature, CpuModel};

    let flat = CpuState::default();
    assert!(FLAT32.is_compatible_with(FLAT32, &flat));
    let physical = CompilerConfig {
        memory_backend: MemoryBackend::PhysicalBus,
        ..FLAT32
    };
    assert!(!physical.is_compatible_with(FLAT32, &flat));
    let restricted = CompilerConfig {
        cpu_model: CpuModel::default().with_feature(CpuFeature::Cmov, false),
        ..FLAT32
    };
    assert!(!FLAT32.is_compatible_with(restricted, &flat));
    assert!(!restricted.is_compatible_with(FLAT32, &flat));
    let unlimited = CompilerConfig {
        instruction_budgets: false,
        ..FLAT32
    };
    assert!(!FLAT32.is_compatible_with(unlimited, &flat));
    assert!(!unlimited.is_compatible_with(FLAT32, &flat));
    assert!(!FLAT32.is_compatible_with(SEGMENTED32, &flat));
    assert!(!FLAT32.is_compatible_with(FLAT32, &CpuState::real_mode()));
    assert!(REAL16.is_compatible_with(REAL16, &CpuState::real_mode()));
    assert!(!REAL16.is_compatible_with(FLAT32, &CpuState::real_mode()));
}

#[test]
fn hosted_cpu_images_require_if_set_and_iopl_zero() {
    let hosted = FLAT32;
    let pc = CompilerConfig::new(
        EntryAssumptions {
            interrupt_flag: None,
            iopl: None,
            ..EntryAssumptions::FLAT32
        },
        MemoryBackend::PhysicalBus,
    );
    // A byte write can replace just part of a constrained multi-byte field.
    let cr0 = std::mem::offset_of!(CpuState, cr0);
    assert!(!hosted.matches_control_write(cr0, &[0]));
    assert!(hosted.matches_control_write(cr0, &[1]));
    assert!(hosted.matches_control_write(cr0 + 1, &[0xff]));

    let same_flags_on_bus = CompilerConfig {
        memory_backend: MemoryBackend::PhysicalBus,
        ..hosted
    };
    let mut cpu = CpuState::default();
    cpu.flags.bytes.if_ = 0xff;
    cpu.flags.bytes.iopl = 0xfc;
    assert!(hosted.is_compatible_with(hosted, &cpu));
    for (interrupts_enabled, iopl) in [(0xfe, 0xfc), (0xff, 0xff)] {
        cpu.flags.bytes.if_ = interrupts_enabled;
        cpu.flags.bytes.iopl = iopl;
        assert!(!hosted.is_compatible_with(hosted, &cpu));
        assert!(!same_flags_on_bus.is_compatible_with(same_flags_on_bus, &cpu));
        assert!(pc.is_compatible_with(pc, &cpu));
    }
}

#[test]
fn mode_segments_and_code_width_constrain_only_the_requested_fields() {
    use crate::{SegmentAssumptions, SegmentDefaultSize};
    let protected16 =
        CpuState::protected_mode(SegmentDefaultSize::Bits16, SegmentDefaultSize::Bits32);
    let real = CpuState::real_mode();
    let flat = CpuState::default();
    let mode_only = EntryAssumptions {
        mode: EntryAssumptions::FLAT32.mode,
        ..EntryAssumptions::LIVE
    };
    let cr4_offset = std::mem::offset_of!(CpuState, cr4);
    let restricted = (flat.cr4 | crate::state::cr4::TIMESTAMP_DISABLE).to_le_bytes();
    let physical = crate::MemoryBackend::PhysicalBus;
    assert!(crate::CompilerConfig::new(mode_only, physical)
        .matches_control_write(cr4_offset, &restricted));
    assert!(
        !crate::CompilerConfig::new(EntryAssumptions::FLAT32, physical)
            .matches_control_write(cr4_offset, &restricted)
    );
    let flat_with_live_mode_and_width = EntryAssumptions {
        segments: Some(SegmentAssumptions::Flat),
        ..EntryAssumptions::LIVE
    };
    for cpu in [real, protected16, flat] {
        assert!(cpu.is_compatible_with(EntryAssumptions::LIVE));
    }
    assert!(protected16.is_compatible_with(flat_with_live_mode_and_width));
    assert!(flat.is_compatible_with(flat_with_live_mode_and_width));
    assert!(!real.is_compatible_with(flat_with_live_mode_and_width));
    for size in [None, Some(SegmentDefaultSize::Bits32)] {
        let entry = EntryAssumptions {
            code_default_size: size,
            ..EntryAssumptions::REAL_MODE
        };
        assert_eq!(real.is_compatible_with(entry), size.is_none());
    }
}
