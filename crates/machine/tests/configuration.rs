use relocade_machine::{ConfigurationError, CpuConfig, Machine, MachineConfig, PagedMemoryMap};
use relocade_wasm_runtime::Backend;
use relocade_x86::{
    Compiler, CpuState, ExecutionProfile, ExecutionTarget, PhysicalMapping, PhysicalMemoryMap,
    SegmentProfile, Segments,
};

fn target() -> ExecutionTarget {
    Compiler::new(ExecutionProfile::Real16).target()
}

fn cpu() -> CpuState {
    CpuState {
        segments: Segments::real_mode(),
        ..CpuState::default()
    }
}

#[test]
fn cpu_must_match_the_execution_profile() {
    assert!(matches!(
        CpuConfig::new(target(), CpuState::default()),
        Err(ConfigurationError::IncompatibleCpu)
    ));
}

#[test]
fn memory_map_must_match_the_execution_profile() {
    let mut backend = Backend::new().unwrap();
    let physical = Machine::new(
        &mut backend,
        MachineConfig::new(0, PhysicalMemoryMap::new([]).unwrap()).unwrap(),
    )
    .unwrap();
    let protected = CpuConfig::new(
        Compiler::new(SegmentProfile::Flat32).target(),
        CpuState::default(),
    )
    .unwrap();
    assert!(matches!(
        physical.bind_cpu(&mut backend, protected),
        Err(relocade_machine::Error::Configuration(
            ConfigurationError::MemoryMapMismatch
        ))
    ));
    assert_eq!(physical.cpu_count(), 0);
    let paged = Machine::new(
        &mut backend,
        MachineConfig::new(0, PagedMemoryMap::default()).unwrap(),
    )
    .unwrap();
    assert!(matches!(
        paged.bind_cpu(&mut backend, CpuConfig::new(target(), cpu()).unwrap()),
        Err(relocade_machine::Error::Configuration(
            ConfigurationError::MemoryMapMismatch
        ))
    ));
    assert_eq!(paged.cpu_count(), 0);
}

#[test]
fn backing_capacity_is_independent_of_mapped_guest_addresses() {
    let map = PhysicalMemoryMap::new([(
        0xf0000..=0xfffff,
        PhysicalMapping::Rom { backing_offset: 0 },
    )])
    .unwrap();
    let config = MachineConfig::new(0x20000, map).unwrap();
    assert_eq!(config.memory_size(), 0x20000);
    assert_eq!(
        match config.memory_map() {
            relocade_machine::MemoryMap::Physical(map) => map.get(0xfffff),
            _ => unreachable!(),
        },
        PhysicalMapping::Rom {
            backing_offset: 0xffff
        }
    );
}

#[test]
fn validation_includes_rom_backing_hidden_by_a20() {
    let map = PhysicalMemoryMap::new([(
        0x100000..=0x100fff,
        PhysicalMapping::Rom {
            backing_offset: 0x2000,
        },
    )])
    .unwrap();
    assert!(matches!(
        MachineConfig::new(0x2fff, map),
        Err(ConfigurationError::InsufficientMemory)
    ));
}

#[test]
fn backing_can_reach_the_end_of_the_32_bit_address_space() {
    let map = || {
        PhysicalMemoryMap::new([(
            0..=0xfff,
            PhysicalMapping::Ram {
                backing_offset: 0xfffff000,
            },
        )])
        .unwrap()
    };
    assert!(MachineConfig::new(1 << 32, map()).is_ok());
    assert!(matches!(
        MachineConfig::new((1 << 32) - 1, map()),
        Err(ConfigurationError::InsufficientMemory)
    ));
    assert!(matches!(
        MachineConfig::new((1 << 32) + 1, map()),
        Err(ConfigurationError::InvalidMemorySize)
    ));
}
