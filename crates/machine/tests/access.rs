use support::Configuration;
mod support;
use relocade_machine::{ConfigurationError, MemoryError};
use relocade_x86::{
    Compiler, CpuState, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap, Segment,
    SegmentRegister, Segments, StatusSourceState,
};
use std::rc::Rc;
use support::Storage;

fn configuration(map: PhysicalMemoryMap) -> (Configuration, Storage) {
    let model =
        relocade_x86::CpuModel::default().with_feature(relocade_x86::CpuFeature::Cmov, false);
    let cpu = CpuState {
        segments: Segments::real_mode(),
        ..CpuState::default()
    };
    let config = support::configuration(
        Compiler::new(ExecutionProfile::Real16)
            .with_cpu_model(model)
            .target(),
        cpu,
        0x3000,
        map,
    )
    .unwrap();
    let storage = Storage::new(&config.cpu, &config.machine);
    (config, storage)
}

#[test]
fn register_access_is_live_and_preserves_aliases() {
    let (config, mut storage) = configuration(PhysicalMemoryMap::new([]).unwrap());
    let context = Rc::clone(&storage.cpu.context);
    {
        let mut machine = storage.context(&config);
        let mut cpu = machine.cpu();
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
        *storage.cpu.cpu_reads.borrow(),
        [(28, 4), (28, 2), (28, 1), (29, 1)]
    );
    assert_eq!(config.cpu.cpu().eip, 0);
}

#[test]
fn snapshots_preserve_the_whole_cpu_without_changing_execution_controls() {
    let (config, mut storage) = configuration(PhysicalMemoryMap::new([]).unwrap());
    let context = Rc::clone(&storage.cpu.context);
    let mut machine = storage.context(&config);
    let mut cpu = machine.cpu();
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
    let (config, mut storage) = configuration(PhysicalMemoryMap::new([]).unwrap());
    let mut machine = storage.context(&config);
    let before = machine.cpu().snapshot();
    let mut snapshot = before;
    snapshot.registers.eax = 0x1234_5678;
    snapshot.segments.cs.base = 1;
    assert_eq!(
        machine.cpu().restore(&snapshot),
        Err(ConfigurationError::IncompatibleCpu)
    );
    assert_eq!(machine.cpu().snapshot(), before);
}

#[test]
fn segment_edits_preserve_the_execution_profile() {
    let (config, mut storage) = configuration(PhysicalMemoryMap::new([]).unwrap());
    let mut machine = storage.context(&config);
    let mut cpu = machine.cpu();
    let ds = SegmentRegister::real_mode(Segment::Ds, 0x1234);
    cpu.set_segment(Segment::Ds, ds);
    assert_eq!(cpu.segment(Segment::Ds), ds);
}

#[test]
#[should_panic(expected = "segment Ds violates execution profile Real16")]
fn incompatible_segment_edits_panic() {
    let (config, mut storage) = configuration(PhysicalMemoryMap::new([]).unwrap());
    let mut machine = storage.context(&config);
    let mut ds = SegmentRegister::real_mode(Segment::Ds, 0x1234);
    ds.base = 0;
    machine.cpu().set_segment(Segment::Ds, ds);
}

#[test]
fn carry_edits_preserve_other_lazy_flags_and_unrelated_cpu_state() {
    let (config, mut storage) = configuration(PhysicalMemoryMap::new([]).unwrap());
    let mut machine = storage.context(&config);
    let mut snapshot = *config.cpu.cpu();
    snapshot.flags.status_source = StatusSourceState {
        kind: 2, // ADD byte: ff + 1 => CF, PF, AF, ZF.
        left: 0xff,
        right: 1,
        ..StatusSourceState::default()
    };
    snapshot.flags.bytes.df = 1;
    snapshot.registers.eax = 0x1234_5678;
    machine.cpu().restore(&snapshot).unwrap();
    assert!(machine.cpu().carry());
    assert_eq!(machine.cpu().eflags(), 0x457);
    assert_eq!(machine.cpu().snapshot(), snapshot); // Reading does not materialize.
    machine.cpu().set_carry(false);
    assert_eq!(machine.cpu().eflags(), 0x456);
    let edited = machine.cpu().snapshot();
    assert_eq!(edited.flags.status_source.kind, 0);
    assert_eq!(edited.flags.status_source.left, 0xff);
    snapshot.flags = edited.flags;
    assert_eq!(edited, snapshot);
}

#[test]
fn transfers_cross_pages_with_their_guest_access_policy() {
    let map = PhysicalMemoryMap::new([
        (
            0..=0xfff,
            PhysicalMapping::Ram {
                backing_offset: 0x1000,
            },
        ),
        (0x1000..=0x1fff, PhysicalMapping::Rom { backing_offset: 0 }),
        (0x3000..=0x3fff, PhysicalMapping::Mmio),
    ])
    .unwrap();
    let (config, mut storage) = configuration(map);
    let mut machine = storage.context(&config);
    let mut memory = machine.memory();
    memory.write_backing(0, &[0xaa, 0xbb]).unwrap();
    memory.write(0xffe, &[1, 2, 3, 4]).unwrap();
    let mut bytes = [0; 4];
    memory.read(0xffe, &mut bytes).unwrap();
    assert_eq!(bytes, [1, 2, 0xaa, 0xbb]);
    // Scalar accesses use little-endian order across the same RAM/ROM boundary.
    assert_eq!(memory.read_u16(0xfff).unwrap(), 0xaa02);
    memory.write_u16(0xfff, 0x7856).unwrap();
    assert_eq!(memory.read_u8(0xfff).unwrap(), 0x56);
    assert_eq!(memory.read_u16(0xfff).unwrap(), 0xaa56);
    memory.write_u8(0x1000, 0x99).unwrap();
    assert_eq!(memory.read_u8(0x1000).unwrap(), 0xaa);
    memory.write_backing(0xffe, &[0xcc, 0xdd]).unwrap();
    memory.read(0x1ffe, &mut bytes).unwrap();
    assert_eq!(bytes, [0xcc, 0xdd, 0xff, 0xff]);
    memory.write(0x2ffe, &[1, 2, 3, 4]).unwrap();
    memory.read(0x2ffe, &mut bytes).unwrap();
    assert_eq!(bytes, [0xff; 4]);
    memory.read(0x200000, &mut bytes).unwrap();
    assert_eq!(bytes, [0xff; 4]);
}

#[test]
fn a20_aliases_and_enabled_high_memory_use_the_same_backing() {
    for enabled in [false, true] {
        let mut map = PhysicalMemoryMap::new([
            (0..=0xfff, PhysicalMapping::Ram { backing_offset: 0 }),
            (
                0x100000..=0x100fff,
                PhysicalMapping::Ram {
                    backing_offset: 0x2000,
                },
            ),
        ])
        .unwrap();
        map.set_a20_enabled(enabled);
        let (config, mut storage) = configuration(map);
        let mut machine = storage.context(&config);
        let mut memory = machine.memory();
        memory.write(0x100000, &[0x5a]).unwrap();
        let mut byte = [0];
        memory
            .read_backing(if enabled { 0x2000 } else { 0 }, &mut byte)
            .unwrap();
        assert_eq!(byte, [0x5a]);
        let ds = SegmentRegister::real_mode(Segment::Ds, 0xffff);
        memory.read_segment(&ds, 0x10, &mut byte).unwrap();
        assert_eq!(byte, [0x5a]);
        memory.write_segment(&ds, 0x10, &[0x42]).unwrap();
        memory.read(0x100000, &mut byte).unwrap();
        assert_eq!(byte, [0x42]);
    }
}

#[test]
fn segment_transfers_check_the_whole_span_before_copying() {
    let map = PhysicalMemoryMap::new([
        (0xf000..=0xffff, PhysicalMapping::Ram { backing_offset: 0 }),
        (
            0x10000..=0x10fff,
            PhysicalMapping::Ram {
                backing_offset: 0x1000,
            },
        ),
    ])
    .unwrap();
    let (config, mut storage) = configuration(map);
    let mut machine = storage.context(&config);
    let mut memory = machine.memory();
    let ds = SegmentRegister::real_mode(Segment::Ds, 0);
    memory.write(0xfffe, b"ABCD").unwrap();
    for offset in [0xffff, 0x10000, u32::MAX] {
        let mut bytes = [0x55; 2];
        assert_eq!(
            memory.read_segment(&ds, offset, &mut bytes),
            Err(MemoryError::SegmentRange)
        );
        assert_eq!(bytes, [0x55; 2]);
        assert_eq!(
            memory.write_segment(&ds, offset, b"XY"),
            Err(MemoryError::SegmentRange)
        );
    }
    let mut bytes = [0; 4];
    memory.read(0xfffe, &mut bytes).unwrap();
    assert_eq!(&bytes, b"ABCD");
    memory.read_segment(&ds, 0xffff, &mut bytes[..1]).unwrap();
    assert_eq!(bytes[0], b'B');
    memory.write_segment(&ds, 0xfffe, b"XY").unwrap();
    memory.read(0xfffe, &mut bytes).unwrap();
    assert_eq!(&bytes, b"XYCD");
    memory.read_segment(&ds, 0xffff, &mut []).unwrap();
    memory.write_segment(&ds, 0xffff, &[]).unwrap();
}

#[test]
fn real_mode_segment_transfers_allow_cs_writes_and_preserve_rom_policy() {
    let map = PhysicalMemoryMap::new([
        (
            0x12000..=0x12fff,
            PhysicalMapping::Ram { backing_offset: 0 },
        ),
        (
            0x13000..=0x13fff,
            PhysicalMapping::Rom {
                backing_offset: 0x1000,
            },
        ),
    ])
    .unwrap();
    let (config, mut storage) = configuration(map);
    let mut machine = storage.context(&config);
    let mut memory = machine.memory();
    let cs = SegmentRegister::real_mode(Segment::Cs, 0x1234);
    memory.write_backing(0x1000, b"CD").unwrap();
    memory.write_segment(&cs, 0xcbe, b"ABxy").unwrap();
    let mut bytes = [0; 4];
    memory.read_segment(&cs, 0xcbe, &mut bytes).unwrap();
    assert_eq!(&bytes, b"ABCD"); // CS:0CBE translates to 12FFE, crossing into ROM.
}

#[test]
fn invalid_spans_fail_before_any_transfer() {
    let (config, mut storage) = configuration(PhysicalMemoryMap::new([]).unwrap());
    let mut machine = storage.context(&config);
    let mut memory = machine.memory();
    assert_eq!(memory.backing_size(), 0x3000);
    memory.write_backing(0x2fff, &[0x42]).unwrap();
    assert_eq!(
        memory.write_backing(0x2fff, &[1, 2]),
        Err(MemoryError::BackingRange)
    );
    let mut byte = [0];
    memory.read_backing(0x2fff, &mut byte).unwrap();
    assert_eq!(byte, [0x42]);
    let mut bytes = [0x55; 2];
    assert_eq!(
        memory.read(u32::MAX, &mut bytes),
        Err(MemoryError::AddressRange)
    );
    assert_eq!(bytes, [0x55; 2]);
    assert_eq!(
        memory.write(u32::MAX, &[1, 2]),
        Err(MemoryError::AddressRange)
    );
    assert_eq!(memory.read_u16(u32::MAX), Err(MemoryError::AddressRange));
    assert_eq!(
        memory.write_u16(u32::MAX, 0x1234),
        Err(MemoryError::AddressRange)
    );
    memory.read(u32::MAX, &mut byte).unwrap();
    assert_eq!(byte, [0xff]);
}

#[test]
fn real_mode_idtr_edits_preserve_the_runtime_vector_table() {
    let (config, mut storage) = configuration(PhysicalMemoryMap::new([]).unwrap());
    let mut machine = storage.context(&config);
    let before = machine.cpu().snapshot();
    let changed = relocade_x86::DescriptorTableRegister::new(0x1000, 0x3ff);
    assert_eq!(
        machine.cpu().set_idtr(changed),
        Err(ConfigurationError::InvalidTableRegister)
    );
    assert_eq!(machine.cpu().snapshot(), before);
}

#[test]
fn restore_rejects_incompatible_idtr_without_writing() {
    let (config, mut storage) = configuration(PhysicalMemoryMap::new([]).unwrap());
    let mut machine = storage.context(&config);
    let before = machine.cpu().snapshot();
    let mut snapshot = before;
    snapshot.idtr = relocade_x86::DescriptorTableRegister::new(0x1000, 0x3ff);
    assert_eq!(
        machine.cpu().restore(&snapshot),
        Err(ConfigurationError::InvalidTableRegister)
    );
    assert_eq!(machine.cpu().snapshot(), before);
}
