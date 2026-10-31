use relocade_machine::{ConfigurationError, Cpu, CpuConfig, MachineConfig, Memory};
use relocade_x86::{
    Compiler, CpuState, ExecutionProfile, ExecutionTarget, PhysicalMapping, PhysicalMemoryMap,
    SegmentProfile,
};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

fn target() -> ExecutionTarget {
    Compiler::new(ExecutionProfile::Real16).target()
}

#[test]
fn cpu_must_match_the_execution_profile() {
    assert!(matches!(
        CpuConfig::new(target(), CpuState::default()),
        Err(ConfigurationError::IncompatibleCpu)
    ));
}

async fn check_memory_profile() {
    let config = MachineConfig::new(0, PhysicalMemoryMap::new([]).unwrap()).unwrap();
    let protected = CpuConfig::new(
        Compiler::new(SegmentProfile::Flat32).target(),
        CpuState::default(),
    )
    .unwrap();
    assert!(matches!(
        Cpu::new(Memory::new(config).unwrap(), protected, Default::default()).await,
        Err(relocade_machine::Error::Configuration(
            ConfigurationError::UnsupportedProfile
        ))
    ));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn memory_map_must_match_the_execution_profile() {
    pollster::block_on(check_memory_profile());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn memory_map_must_match_the_execution_profile() {
    check_memory_profile().await;
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
        config.memory_map().get(0xfffff),
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
