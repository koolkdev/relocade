use relocade_machine::{ConfigurationError, Cpu, CpuConfig, MachineConfig, Memory};
use relocade_x86::{
    Compiler, CpuState, ExecutionProfile, ExecutionTarget, MemoryBackend, PhysicalMapping,
    PhysicalMemoryMap, SegmentDefaultSize, SegmentProfile,
};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_dedicated_worker);

fn target() -> ExecutionTarget {
    Compiler::new(ExecutionProfile::Real16).target()
}

#[test]
fn physical_pc_admits_canonical_real_mode_and_protected_ring_three() {
    use SegmentDefaultSize::{Bits16, Bits32};
    let mut wide_code = CpuState::protected_mode(Bits32, Bits16);
    wide_code.segments.cs.big = 0x80; // Every nonzero cache byte represents CS.D=1.
    for cpu in [
        CpuState::real_mode(),
        CpuState::protected_mode(Bits16, Bits32),
        wide_code,
    ] {
        assert!(CpuConfig::pc(cpu).is_ok());
    }
    // Setting PE on the reset real-mode image leaves CS at ring zero.
    let mut cpu = CpuState::real_mode();
    cpu.cr0 |= 1;
    assert!(matches!(
        CpuConfig::pc(cpu),
        Err(ConfigurationError::IncompatibleCpu)
    ));
}

async fn check_memory_profile() {
    let config = MachineConfig::new(0, PhysicalMemoryMap::new([]).unwrap()).unwrap();
    let memory = Memory::new(config).unwrap();
    for (profile, backend, state, expected) in [
        (
            SegmentProfile::Flat32.into(),
            MemoryBackend::PhysicalBus,
            CpuState::default(),
            ConfigurationError::UnsupportedMode,
        ),
        (
            ExecutionProfile::Real16,
            MemoryBackend::HostPageTable,
            CpuState::real_mode(),
            ConfigurationError::UnsupportedMemoryBackend,
        ),
    ] {
        let target = ExecutionTarget {
            memory_backend: backend,
            ..Compiler::new(profile).target()
        };
        let cpu = CpuConfig::new(target, state).unwrap();
        assert!(matches!(
            Cpu::new(memory.clone(), cpu, Default::default()).await,
            Err(relocade_machine::Error::Configuration(error)) if error == expected
        ));
    }
    let real = CpuConfig::new(target(), CpuState::real_mode()).unwrap();
    Cpu::new(memory, real, Default::default()).await.unwrap();
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
