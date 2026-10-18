use wasm86_machine::{ConfigurationError, MachineConfig};
use wasm86_x86::{
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
fn cpu_and_profile_must_match_the_physical_configuration() {
    assert!(matches!(
        MachineConfig::new(
            target(),
            CpuState::default(),
            0,
            PhysicalMemoryMap::new([]).unwrap()
        ),
        Err(ConfigurationError::IncompatibleCpu)
    ));
    assert!(matches!(
        MachineConfig::new(
            Compiler::new(SegmentProfile::Flat32).target(),
            CpuState::default(),
            0,
            PhysicalMemoryMap::new([]).unwrap(),
        ),
        Err(ConfigurationError::UnsupportedProfile)
    ));
}

#[test]
fn backing_capacity_is_independent_of_mapped_guest_addresses() {
    let map = PhysicalMemoryMap::new([(
        0xf0000..=0xfffff,
        PhysicalMapping::Rom { backing_offset: 0 },
    )])
    .unwrap();
    let config = MachineConfig::new(target(), cpu(), 0x20000, map).unwrap();
    assert_eq!(config.memory_size(), 0x20000);
    assert_eq!(
        config.memory_map().get(0xfffff),
        PhysicalMapping::Rom {
            backing_offset: 0xffff
        }
    );
    assert_eq!(config.execution_target(), target());
    assert!(target().profile.is_compatible_with(&config.cpu().segments));
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
        MachineConfig::new(target(), cpu(), 0x2fff, map),
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
    assert!(MachineConfig::new(target(), cpu(), 1 << 32, map()).is_ok());
    assert!(matches!(
        MachineConfig::new(target(), cpu(), (1 << 32) - 1, map()),
        Err(ConfigurationError::InsufficientMemory)
    ));
    assert!(matches!(
        MachineConfig::new(target(), cpu(), (1 << 32) + 1, map()),
        Err(ConfigurationError::InvalidMemorySize)
    ));
}
