use relocade_machine::{ConfigurationError, Cpu, CpuConfig, MachineConfig, Memory};
use relocade_x86::{
    CompilerConfig, CpuState, EntryAssumptions, MemoryBackend, PhysicalMapping, PhysicalMemoryMap,
};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

fn pc_compiler_config() -> CompilerConfig {
    CompilerConfig::new(EntryAssumptions::REAL_MODE, MemoryBackend::PhysicalBus)
}

#[test]
fn cpu_must_match_the_entry_assumptions() {
    assert!(matches!(
        CpuConfig::new(pc_compiler_config(), CpuState::default()),
        Err(ConfigurationError::IncompatibleCpu)
    ));
    // Canonical real caches cannot hide protected control state.
    let mut cpu = CpuState::real_mode();
    cpu.cr0 |= 1;
    assert!(matches!(
        CpuConfig::new(pc_compiler_config(), cpu),
        Err(ConfigurationError::IncompatibleCpu)
    ));
}

async fn check_pc_configuration() {
    let machine = MachineConfig::new(0, PhysicalMemoryMap::new([]).unwrap()).unwrap();
    let memory = Memory::new(machine).unwrap();
    for (entry, backend, state, expected) in [
        (
            EntryAssumptions::FLAT32,
            MemoryBackend::PhysicalBus,
            CpuState::default(),
            ConfigurationError::UnsupportedMode,
        ),
        (
            EntryAssumptions {
                code_default_size: None,
                ..EntryAssumptions::REAL_MODE
            },
            MemoryBackend::PhysicalBus,
            CpuState::real_mode(),
            ConfigurationError::UnsupportedMode,
        ),
        (
            EntryAssumptions::REAL_MODE,
            MemoryBackend::HostPageTable,
            CpuState::real_mode(),
            ConfigurationError::UnsupportedMemoryBackend,
        ),
    ] {
        let config = CompilerConfig::new(entry, backend);
        let cpu = CpuConfig::new(config, state).unwrap();
        assert!(matches!(
            Cpu::new(memory.clone(), cpu, Default::default()).await,
            Err(relocade_machine::Error::Configuration(error)) if error == expected
        ));
    }
    let real = CpuConfig::new(pc_compiler_config(), CpuState::real_mode()).unwrap();
    Cpu::new(memory, real, Default::default()).await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn pc_requires_real_mode_and_the_physical_bus() {
    pollster::block_on(check_pc_configuration());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn pc_requires_real_mode_and_the_physical_bus() {
    check_pc_configuration().await;
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
