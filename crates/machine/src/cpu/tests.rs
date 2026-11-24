use super::CpuStateAccess;
use crate::{ConfigurationError, CpuConfig};
use relocade_wasm_runtime::{SharedMemory, WasmSharedMemory};
use relocade_x86::{
    CompilerConfig, CpuState, EntryAssumptions, ExecutionContext, MemoryBackend, Segment,
    SegmentRegister, Segments, StatusSourceState,
};

fn cpu_memory(cpu: &CpuConfig) -> SharedMemory {
    let memory = SharedMemory::new(1, 1).unwrap();
    memory.view().write(
        0,
        &ExecutionContext {
            cpu: *cpu.cpu(),
            cpu_model: cpu.compiler_config().cpu_model,
            interrupt_requested: 1,
            ..ExecutionContext::default()
        }
        .to_bytes(),
    );
    memory
}

fn read<const N: usize>(memory: &SharedMemory, address: u32) -> [u8; N] {
    let mut bytes = [0; N];
    memory.view().read(address, &mut bytes);
    bytes
}

fn configuration() -> CpuConfig {
    let model =
        relocade_x86::CpuModel::default().with_feature(relocade_x86::CpuFeature::Cmov, false);
    CpuConfig::new(
        CompilerConfig {
            cpu_model: model,
            ..CompilerConfig::new(EntryAssumptions::REAL_MODE, MemoryBackend::PhysicalBus)
        },
        CpuState::real_mode(),
    )
    .unwrap()
}

#[test]
fn register_access_is_live_and_preserves_aliases() {
    let config = configuration();
    let storage = cpu_memory(&config);
    let context = storage.clone();
    {
        let cpu = CpuStateAccess::new(&config, storage);
        cpu.set_eax(0x1234_5678);
        cpu.set_ax(0xabcd);
        cpu.set_al(0xef);
        cpu.set_ah(0x98);
        // Observe writes through another owner: no drop or flush is needed.
        assert_eq!(read::<4>(&context, 28), [0xef, 0x98, 0x34, 0x12]);
        assert_eq!(cpu.eax(), 0x1234_98ef);
        context.view().store(28, 0x42_u8);
        assert_eq!(cpu.ax(), 0x9842);
        assert_eq!(cpu.al(), 0x42);
        assert_eq!(cpu.ah(), 0x98);
        cpu.set_eip(0x100);
        assert_eq!(read::<4>(&context, 60), [0, 1, 0, 0]);
        assert_eq!(*cpu.reads.borrow(), [(28, 4), (28, 2), (28, 1), (29, 1)]);
    }
    assert_eq!(config.cpu().eip, 0);
}

#[test]
fn saved_registers_restore_only_the_named_views() {
    use super::registers as reg;

    let config = configuration();
    let cpu = CpuStateAccess::new(&config, cpu_memory(&config));
    cpu.set_eax(0x1234_5678);
    cpu.set_ebx(0x1122_3344);
    cpu.set_ecx(0x5566_7788);
    let ds = SegmentRegister::real_mode(Segment::Ds, 0x1234);
    cpu.set_segment(Segment::Ds, ds);
    cpu.reads.borrow_mut().clear();
    let saved = cpu.save_registers([reg::ax, reg::bh, reg::ecx, reg::segment(Segment::Ds)]);
    assert_eq!(
        cpu.reads
            .borrow()
            .iter()
            .map(|&(_, bytes)| bytes)
            .collect::<Vec<_>>(),
        [2, 1, 4, size_of::<SegmentRegister>()]
    );

    cpu.set_eax(0xaabb_ccdd);
    cpu.set_ebx(0x99aa_bbcc);
    cpu.set_ecx(0xdead_beef);
    cpu.set_segment(Segment::Ds, SegmentRegister::real_mode(Segment::Ds, 0xabcd));
    cpu.set_eflags(0x647);
    cpu.set_eip(0x3210);
    cpu.store(std::mem::offset_of!(CpuState, instruction_count), 99_u64);
    let mut expected = cpu.snapshot();
    expected.registers.eax = 0xaabb_5678;
    expected.registers.ebx = 0x99aa_33cc;
    expected.registers.ecx = 0x5566_7788;
    expected.segments.ds = ds;
    saved.restore();
    assert_eq!(cpu.snapshot(), expected);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn incompatible_saved_segment_leaves_all_registers_unchanged() {
    use super::registers as reg;
    use std::panic::{catch_unwind, AssertUnwindSafe};

    let config = CpuConfig::pc(CpuState::default()).unwrap();
    let cpu = CpuStateAccess::new(&config, cpu_memory(&config));
    cpu.set_ax(0x1234);
    let saved = cpu.save_registers([reg::ax, reg::segment(Segment::Ds)]);

    let real = CpuState::real_mode();
    cpu.restore(&real).unwrap();
    assert!(catch_unwind(AssertUnwindSafe(|| saved.restore())).is_err());
    assert_eq!(cpu.snapshot(), real);
}

#[test]
fn restoring_architectural_state_preserves_retirement_and_execution_controls() {
    let config = configuration();
    let storage = cpu_memory(&config);
    let context = storage.clone();
    let cpu = CpuStateAccess::new(&config, storage);
    let snapshot = CpuState {
        cr0: 0x30,
        cr4: 0x600,
        flags: relocade_x86::FlagsState {
            bytes: relocade_x86::FlagBytes {
                vm: 0,
                ..CpuState::filled(0xa5).flags.bytes
            },
            ..CpuState::filled(0xa5).flags
        },
        segments: Segments::real_mode(),
        idtr: relocade_x86::DescriptorTableRegister::REAL_MODE_IDTR,
        ldtr: relocade_x86::SystemSegmentRegister::default(),
        tr: relocade_x86::SystemSegmentRegister::default(),
        ..CpuState::filled(0xa5)
    };
    cpu.store(std::mem::offset_of!(CpuState, instruction_count), 99_u64);
    cpu.restore(&snapshot).unwrap();
    let mut expected = snapshot;
    expected.instruction_count = 99;
    assert_eq!(cpu.snapshot(), expected);
    assert_eq!(read::<8>(&context, 544), [0xfd, 1, 0, 0, 1, 0, 0, 0]);
}

#[test]
fn live_mode_and_width_queries_read_published_state() {
    use relocade_x86::{CpuMode, PrivilegeLevel, SegmentDefaultSize::*};
    let config = CpuConfig::new(
        CompilerConfig::new(EntryAssumptions::SEGMENTED16, MemoryBackend::HostPageTable),
        CpuState::protected_mode(Bits16, Bits32),
    )
    .unwrap();
    let cpu = CpuStateAccess::new(&config, cpu_memory(&config));
    assert_eq!(cpu.mode(), CpuMode::Protected);
    assert_eq!(cpu.privilege_level(), PrivilegeLevel::Ring3);
    assert_eq!(cpu.code_default_size(), Bits16);
    assert_eq!(cpu.stack_address_size(), Bits32);
    assert_eq!(cpu.cr0(), 0x31);
    assert_eq!(cpu.cr4(), 0x600);
    let mut ss = cpu.segment(Segment::Ss);
    ss.big = 0;
    cpu.set_segment(Segment::Ss, ss);
    assert_eq!(cpu.stack_address_size(), Bits16);
    assert_eq!(cpu.code_default_size(), Bits16);
}

#[test]
fn restore_rejects_incompatible_control_state_without_writing() {
    for config in [
        configuration(),
        CpuConfig::new(
            CompilerConfig::new(EntryAssumptions::FLAT32, MemoryBackend::HostPageTable),
            CpuState::default(),
        )
        .unwrap(),
    ] {
        let cpu = CpuStateAccess::new(&config, cpu_memory(&config));
        let before = cpu.snapshot();
        for change in [
            (|cpu: &mut CpuState| cpu.cr0 ^= 1) as fn(&mut CpuState),
            |cpu| cpu.cr0 |= 1 << 31,
            |cpu| cpu.cr4 |= 1 << 2,
            |cpu| cpu.flags.bytes.vm = 1,
        ] {
            let mut proposed = before;
            proposed.registers.eax = 0x1234;
            change(&mut proposed);
            assert_eq!(
                cpu.restore(&proposed),
                Err(ConfigurationError::IncompatibleCpu)
            );
            assert_eq!(cpu.snapshot(), before);
        }
    }
}

// Browser Wasm aborts on panic, so post-panic state checks run natively.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn incompatible_control_and_privilege_edits_leave_state_unchanged() {
    use std::panic::{catch_unwind, AssertUnwindSafe};

    for config in [
        configuration(),
        CpuConfig::new(
            CompilerConfig::new(EntryAssumptions::FLAT32, MemoryBackend::HostPageTable),
            CpuState::default(),
        )
        .unwrap(),
    ] {
        let cpu = CpuStateAccess::new(&config, cpu_memory(&config));
        let before = cpu.snapshot();
        assert!(catch_unwind(AssertUnwindSafe(|| cpu.set_eflags(0x202 | (1 << 17)))).is_err());
        assert_eq!(cpu.snapshot(), before);
    }
    let config = CpuConfig::new(
        CompilerConfig::new(EntryAssumptions::FLAT32, MemoryBackend::HostPageTable),
        CpuState::default(),
    )
    .unwrap();
    let cpu = CpuStateAccess::new(&config, cpu_memory(&config));
    let before = cpu.snapshot();
    assert!(catch_unwind(AssertUnwindSafe(|| cpu.set_interrupts_enabled(false))).is_err());
    assert_eq!(cpu.snapshot(), before);
    for flags in [0x3, 0x1203] {
        assert!(catch_unwind(AssertUnwindSafe(|| cpu.set_eflags(flags))).is_err());
        assert_eq!(cpu.snapshot(), before);
    }
    let mut cs = before.segments.cs;
    cs.selector &= !3;
    assert!(catch_unwind(AssertUnwindSafe(|| cpu.set_segment(Segment::Cs, cs))).is_err());
    assert_eq!(cpu.snapshot(), before);
}

#[test]
fn hosted_cpu_admission_and_restore_require_the_interrupt_flag_guarantees() {
    let compiler_config =
        CompilerConfig::new(EntryAssumptions::FLAT32, MemoryBackend::HostPageTable);
    let config = CpuConfig::new(compiler_config, CpuState::default()).unwrap();
    let cpu = CpuStateAccess::new(&config, cpu_memory(&config));
    let before = cpu.snapshot();
    let mut proposed = before;
    proposed.flags.bytes.if_ = 0;
    assert!(matches!(
        CpuConfig::new(compiler_config, proposed),
        Err(ConfigurationError::IncompatibleCpu)
    ));
    assert_eq!(
        cpu.restore(&proposed),
        Err(ConfigurationError::IncompatibleCpu)
    );
    assert_eq!(cpu.snapshot(), before);
}

#[test]
fn protected_pc_allows_host_edits_to_if_and_iopl() {
    let compiler_config = CompilerConfig::new(
        EntryAssumptions {
            interrupt_flag: None,
            iopl: None,
            ..EntryAssumptions::FLAT32
        },
        MemoryBackend::PhysicalBus,
    );
    let config = CpuConfig::new(compiler_config, CpuState::default()).unwrap();
    let cpu = CpuStateAccess::new(&config, cpu_memory(&config));
    cpu.set_eflags(0x3202);
    cpu.set_interrupts_enabled(false);
    assert_eq!(cpu.eflags(), 0x3002);
    let snapshot = cpu.snapshot();
    assert!(CpuConfig::new(compiler_config, snapshot).is_ok());
    cpu.set_eflags(0x202);
    cpu.restore(&snapshot).unwrap();
    assert_eq!(cpu.eflags(), 0x3002);
}

#[test]
fn interrupt_control_preserves_lazy_flags() {
    let config = configuration();
    let cpu = CpuStateAccess::new(&config, cpu_memory(&config));
    let mut state = cpu.snapshot();
    state.flags.status_source = StatusSourceState {
        kind: 9,
        left: 7,
        right: 8,
        reserved: [0; 3],
    };
    cpu.restore(&state).unwrap();
    cpu.set_interrupts_enabled(true);
    state.flags.bytes.if_ = 1;
    assert!(cpu.interrupts_enabled());
    assert_eq!(cpu.snapshot(), state);
}

#[test]
fn restore_rejects_incompatible_segments_without_writing() {
    let config = configuration();
    let storage = cpu_memory(&config);
    let cpu = CpuStateAccess::new(&config, storage);
    let before = cpu.snapshot();
    let mut snapshot = before;
    snapshot.registers.eax = 0x1234_5678;
    snapshot.segments.cs.base = 1;
    assert_eq!(
        cpu.restore(&snapshot),
        Err(ConfigurationError::IncompatibleCpu)
    );
    assert_eq!(cpu.snapshot(), before);
}

#[test]
fn segment_edits_preserve_cpu_admission() {
    let config = configuration();
    let storage = cpu_memory(&config);
    let cpu = CpuStateAccess::new(&config, storage);
    let ds = SegmentRegister::real_mode(Segment::Ds, 0x1234);
    cpu.set_segment(Segment::Ds, ds);
    assert_eq!(cpu.segment(Segment::Ds), ds);
}

#[test]
#[should_panic(expected = "segment Ds violates CPU admission")]
fn incompatible_segment_edits_panic() {
    let config = configuration();
    let storage = cpu_memory(&config);
    let cpu = CpuStateAccess::new(&config, storage);
    let mut ds = SegmentRegister::real_mode(Segment::Ds, 0x1234);
    ds.base = 0;
    cpu.set_segment(Segment::Ds, ds);
}

#[test]
fn carry_edits_preserve_other_lazy_flags_and_unrelated_cpu_state() {
    let config = configuration();
    let storage = cpu_memory(&config);
    let cpu = CpuStateAccess::new(&config, storage);
    let mut snapshot = *config.cpu();
    snapshot.flags.status_source = StatusSourceState {
        kind: 2, // ADD byte: ff + 1 => CF, PF, AF, ZF.
        left: 0xff,
        right: 1,
        ..StatusSourceState::default()
    };
    snapshot.flags.bytes.df = 1;
    snapshot.registers.eax = 0x1234_5678;
    cpu.restore(&snapshot).unwrap();
    assert!(cpu.carry());
    assert_eq!(cpu.eflags(), 0x457);
    assert_eq!(cpu.snapshot(), snapshot); // Reading does not materialize.
    cpu.set_carry(false);
    assert_eq!(cpu.eflags(), 0x456);
    let edited = cpu.snapshot();
    assert_eq!(edited.flags.status_source.kind, 0);
    assert_eq!(edited.flags.status_source.left, 0xff);
    snapshot.flags = edited.flags;
    assert_eq!(edited, snapshot);
}

#[test]
fn real_mode_idtr_edits_preserve_the_runtime_vector_table() {
    let config = configuration();
    let storage = cpu_memory(&config);
    let cpu = CpuStateAccess::new(&config, storage);
    let before = cpu.snapshot();
    let changed = relocade_x86::DescriptorTableRegister::new(0x1000, 0x3ff);
    assert_eq!(
        cpu.set_idtr(changed),
        Err(ConfigurationError::InvalidTableRegister)
    );
    assert_eq!(cpu.snapshot(), before);
}

#[test]
fn restore_rejects_incompatible_idtr_without_writing() {
    let config = configuration();
    let storage = cpu_memory(&config);
    let cpu = CpuStateAccess::new(&config, storage);
    let before = cpu.snapshot();
    let mut snapshot = before;
    snapshot.idtr = relocade_x86::DescriptorTableRegister::new(0x1000, 0x3ff);
    assert_eq!(
        cpu.restore(&snapshot),
        Err(ConfigurationError::InvalidTableRegister)
    );
    assert_eq!(cpu.snapshot(), before);
}
