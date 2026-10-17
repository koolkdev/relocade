use wasm86_machine::{
    ConfigurationError, Environment, GuestMemory, Machine, MemoryAccess, MemoryError,
};
use wasm86_x86::{
    Compiler, CpuState, ExecutionProfile, PagedMemoryMap, PhysicalMapping, PhysicalMemoryMap,
    SegmentProfile, Segments,
};

#[derive(Default)]
struct Backing {
    bytes: Vec<u8>,
    published: Vec<Vec<u8>>,
}
impl GuestMemory for Backing {
    fn byte_len(&self) -> u64 {
        self.bytes.len() as u64
    }
    fn read(&self, offset: u32, bytes: &mut [u8]) {
        bytes.copy_from_slice(&self.bytes[offset as usize..][..bytes.len()]);
    }
    fn write(&mut self, offset: u32, bytes: &[u8]) {
        self.bytes[offset as usize..][..bytes.len()].copy_from_slice(bytes);
    }
    fn publish_map(&mut self, bytes: &[u8]) {
        self.published.push(bytes.to_vec());
    }
}
fn real_cpu() -> CpuState {
    CpuState {
        segments: Segments::real_mode(),
        ..CpuState::default()
    }
}

#[test]
fn validates_cpu_and_memory_against_the_explicit_target() {
    let real = Compiler::new(ExecutionProfile::Real16).target();
    let flat = Compiler::new(SegmentProfile::Flat32).target();
    assert!(matches!(
        Machine::new(real, real_cpu(), PagedMemoryMap::default(), ()),
        Err(ConfigurationError::IncompatibleMemory)
    ));
    assert!(matches!(
        Machine::new(
            flat,
            CpuState::default(),
            PhysicalMemoryMap::new([]).unwrap(),
            ()
        ),
        Err(ConfigurationError::IncompatibleMemory)
    ));
    assert!(matches!(
        Machine::new(
            real,
            CpuState::default(),
            PhysicalMemoryMap::new([]).unwrap(),
            ()
        ),
        Err(ConfigurationError::IncompatibleCpu)
    ));
    assert!(Machine::new(real, real_cpu(), PhysicalMemoryMap::new([]).unwrap(), ()).is_ok());
    assert!(Machine::new(flat, CpuState::default(), PagedMemoryMap::default(), ()).is_ok());
}

#[test]
fn physical_access_observes_a20_aliases_rom_and_open_bus() {
    let map = PhysicalMemoryMap::new([
        (
            0..=0xfff,
            PhysicalMapping::Ram {
                backing_offset: 0x1000,
            },
        ),
        (
            0xf0000..=0xf0fff,
            PhysicalMapping::Rom { backing_offset: 0 },
        ),
        (
            0x100000..=0x100fff,
            PhysicalMapping::Ram {
                backing_offset: 0x2000,
            },
        ),
    ])
    .unwrap();
    assert_eq!(map.required_backing_bytes(), 0x3000);
    let machine = Machine::new(
        Compiler::new(ExecutionProfile::Real16).target(),
        real_cpu(),
        map,
        (),
    )
    .unwrap();
    let (_, mut imports) = machine.into_parts();
    let mut backing = Backing {
        bytes: vec![0; 0x3000],
        ..Backing::default()
    };
    imports.publish_map(&mut backing);
    imports.access(&mut backing, |_, memory| {
        memory.write_backing(0, &[0xa5]).unwrap();
        memory.write_backing(0x1000, &[0x11]).unwrap();
        memory.write_backing(0x2000, &[0x22]).unwrap();
        assert_eq!(
            memory.write_backing(0x3000, &[1]),
            Err(MemoryError::BackingRange)
        );
    });
    let mut byte = [0];
    imports.read(&mut backing, 0x100000, &mut byte).unwrap();
    assert_eq!(byte, [0x11]);
    imports
        .access(&mut backing, |_, memory| memory.set_a20_enabled(true))
        .unwrap();
    imports.read(&mut backing, 0x100000, &mut byte).unwrap();
    assert_eq!(byte, [0x22]);
    imports.write(&mut backing, 0xf0000, &[0]).unwrap();
    imports.read(&mut backing, 0xf0000, &mut byte).unwrap();
    assert_eq!(byte, [0xa5]);
    imports.read(&mut backing, 0x80000, &mut byte).unwrap();
    assert_eq!(byte, [0xff]);
    assert_eq!(backing.published.len(), 2);
    assert_eq!(
        &backing.published[1][PhysicalMemoryMap::A20_MASK_OFFSET..],
        &[0xff; 4]
    );
}

#[test]
fn paged_access_uses_backing_offsets_and_reports_denied_linear_bytes() {
    let mut map = PagedMemoryMap::default();
    map.map(0x4000..=0x4fff, 0x1000, true).unwrap();
    map.map(0x5000..=0x5fff, 0, false).unwrap();
    let machine = Machine::new(
        Compiler::new(SegmentProfile::Flat32).target(),
        CpuState::default(),
        map,
        (),
    )
    .unwrap();
    let (_, mut imports) = machine.into_parts();
    let mut backing = Backing {
        bytes: vec![0; 0x2000],
        ..Backing::default()
    };
    imports.publish_map(&mut backing);
    imports
        .access(&mut backing, |_, memory| {
            memory.write_backing(0, &[0xab, 0xcd])
        })
        .unwrap();
    assert_eq!(
        imports.write(&mut backing, 0x4fff, &[1, 2]),
        Err(MemoryError::ReadOnly { address: 0x5000 })
    );
    let mut bytes = [0; 3];
    imports.read(&mut backing, 0x4fff, &mut bytes).unwrap();
    assert_eq!(bytes, [1, 0xab, 0xcd]);
    assert_eq!(
        imports.read(&mut backing, 0x6000, &mut bytes),
        Err(MemoryError::Unmapped { address: 0x6000 })
    );
}

#[test]
fn device_mapping_changes_are_published_before_returning() {
    struct Device;
    impl Environment for Device {
        fn read_mmio(&mut self, memory: &mut MemoryAccess<'_>, address: u32, bytes: u8) -> u64 {
            assert_eq!((address, bytes), (0, 1));
            memory
                .set_map(
                    PhysicalMemoryMap::new([(
                        0..=0xfff,
                        PhysicalMapping::Ram { backing_offset: 0 },
                    )])
                    .unwrap(),
                )
                .unwrap();
            0x33
        }
    }
    let map = PhysicalMemoryMap::new([
        (0..=0xfff, PhysicalMapping::Mmio),
        (0x1000..=0x1fff, PhysicalMapping::Ram { backing_offset: 0 }),
    ])
    .unwrap();
    let machine = Machine::new(
        Compiler::new(ExecutionProfile::Real16).target(),
        real_cpu(),
        map,
        Device,
    )
    .unwrap();
    let (_, mut imports) = machine.into_parts();
    let mut backing = Backing {
        bytes: vec![0x44; 4096],
        ..Backing::default()
    };
    let mut bytes = [0; 2];
    imports.read(&mut backing, 0, &mut bytes).unwrap();
    assert_eq!(bytes, [0x33, 0x44]);
    assert_eq!(backing.published.len(), 1);
    assert_eq!(&backing.published[0][..8], &[1, 0, 0, 0, 0, 0, 0, 0]);
}
