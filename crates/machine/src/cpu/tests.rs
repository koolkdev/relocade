use super::CpuView;
use crate::{ConfigurationError, CpuConfig, MemoryAccess, MemoryStorage};
use relocade_x86::{
    Compiler, CpuState, ExecutionContext, ExecutionProfile, Segment, SegmentRegister, Segments,
    StatusSourceState,
};
use std::{cell::RefCell, rc::Rc};

struct CpuImage {
    context: Rc<RefCell<[u8; ExecutionContext::BYTE_LEN]>>,
    cpu_reads: RefCell<Vec<(u32, usize)>>,
}

impl CpuImage {
    fn new(cpu: &CpuConfig) -> Self {
        Self {
            context: Rc::new(RefCell::new(
                ExecutionContext {
                    cpu: *cpu.cpu(),
                    instruction_target: 998,
                    cpu_model: cpu.execution_target().cpu_model,
                }
                .to_bytes(),
            )),
            cpu_reads: RefCell::new(Vec::new()),
        }
    }
}

impl MemoryStorage for CpuImage {
    fn read(&self, offset: u32, bytes: &mut [u8]) {
        self.cpu_reads.borrow_mut().push((offset, bytes.len()));
        bytes.copy_from_slice(&self.context.borrow()[offset as usize..][..bytes.len()]);
    }
    fn write(&mut self, offset: u32, bytes: &[u8]) {
        self.context.borrow_mut()[offset as usize..][..bytes.len()].copy_from_slice(bytes);
    }
}

fn configuration() -> CpuConfig {
    let model =
        relocade_x86::CpuModel::default().with_feature(relocade_x86::CpuFeature::Cmov, false);
    CpuConfig::new(
        Compiler::new(ExecutionProfile::Real16)
            .with_cpu_model(model)
            .target(),
        CpuState {
            segments: Segments::real_mode(),
            ..CpuState::default()
        },
    )
    .unwrap()
}

#[test]
fn register_access_is_live_and_preserves_aliases() {
    let config = configuration();
    let mut storage = CpuImage::new(&config);
    let context = Rc::clone(&storage.context);
    {
        let mut cpu = CpuView::new(&config, MemoryAccess::from_storage(&mut storage));
        cpu.set_eax(0x1234_5678);
        cpu.set_ax(0xabcd);
        cpu.set_al(0xef);
        cpu.set_ah(0x98);
        // Observe writes while the view is still borrowed: no drop/flush is needed.
        assert_eq!(&context.borrow()[28..32], &[0xef, 0x98, 0x34, 0x12]);
        assert_eq!(cpu.eax(), 0x1234_98ef);
        context.borrow_mut()[28] = 0x42;
        assert_eq!(cpu.ax(), 0x9842);
        assert_eq!(cpu.al(), 0x42);
        assert_eq!(cpu.ah(), 0x98);
        cpu.set_eip(0x100);
        assert_eq!(&context.borrow()[60..64], &[0, 1, 0, 0]);
    }
    assert_eq!(
        *storage.cpu_reads.borrow(),
        [(28, 4), (28, 2), (28, 1), (29, 1)]
    );
    assert_eq!(config.cpu().eip, 0);
}

#[test]
fn snapshots_preserve_the_whole_cpu_without_changing_execution_controls() {
    let config = configuration();
    let mut storage = CpuImage::new(&config);
    let context = Rc::clone(&storage.context);
    let mut cpu = CpuView::new(&config, MemoryAccess::from_storage(&mut storage));
    let snapshot = CpuState {
        segments: Segments::real_mode(),
        idtr: relocade_x86::DescriptorTableRegister::REAL_MODE_IDTR,
        ldtr: relocade_x86::SystemSegmentRegister::default(),
        tr: relocade_x86::SystemSegmentRegister::default(),
        ..CpuState::filled(0xa5)
    };
    cpu.restore(&snapshot).unwrap();
    assert_eq!(cpu.snapshot(), snapshot);
    assert_eq!(&context.borrow()[536..544], &998_u64.to_le_bytes());
    assert_eq!(&context.borrow()[544..], &[0xfd, 1, 0, 0, 0, 0, 0, 0]);
}

#[test]
fn restore_rejects_incompatible_segments_without_writing() {
    let config = configuration();
    let mut storage = CpuImage::new(&config);
    let mut cpu = CpuView::new(&config, MemoryAccess::from_storage(&mut storage));
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
fn segment_edits_preserve_the_execution_profile() {
    let config = configuration();
    let mut storage = CpuImage::new(&config);
    let mut cpu = CpuView::new(&config, MemoryAccess::from_storage(&mut storage));
    let ds = SegmentRegister::real_mode(Segment::Ds, 0x1234);
    cpu.set_segment(Segment::Ds, ds);
    assert_eq!(cpu.segment(Segment::Ds), ds);
}

#[test]
#[should_panic(expected = "segment Ds violates execution profile Real16")]
fn incompatible_segment_edits_panic() {
    let config = configuration();
    let mut storage = CpuImage::new(&config);
    let mut cpu = CpuView::new(&config, MemoryAccess::from_storage(&mut storage));
    let mut ds = SegmentRegister::real_mode(Segment::Ds, 0x1234);
    ds.base = 0;
    cpu.set_segment(Segment::Ds, ds);
}

#[test]
fn carry_edits_preserve_other_lazy_flags_and_unrelated_cpu_state() {
    let config = configuration();
    let mut storage = CpuImage::new(&config);
    let mut cpu = CpuView::new(&config, MemoryAccess::from_storage(&mut storage));
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
    let mut storage = CpuImage::new(&config);
    let mut cpu = CpuView::new(&config, MemoryAccess::from_storage(&mut storage));
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
    let mut storage = CpuImage::new(&config);
    let mut cpu = CpuView::new(&config, MemoryAccess::from_storage(&mut storage));
    let before = cpu.snapshot();
    let mut snapshot = before;
    snapshot.idtr = relocade_x86::DescriptorTableRegister::new(0x1000, 0x3ff);
    assert_eq!(
        cpu.restore(&snapshot),
        Err(ConfigurationError::InvalidTableRegister)
    );
    assert_eq!(cpu.snapshot(), before);
}
