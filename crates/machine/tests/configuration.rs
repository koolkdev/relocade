use relocade_machine::{ConfigurationError, Cpu, CpuConfig, MachineConfig, Memory};
use relocade_x86::{
    CompilerConfig, CpuState, EntryAssumptions, MemoryBackend, PhysicalMapping, PhysicalMemoryMap,
    SegmentDefaultSize,
};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_dedicated_worker);

fn pc_compiler_config() -> CompilerConfig {
    CompilerConfig::new(EntryAssumptions::REAL_MODE, MemoryBackend::PhysicalBus)
}

#[test]
fn physical_pc_admits_canonical_real_mode_and_protected_ring_three() {
    use SegmentDefaultSize::{Bits16, Bits32};
    let mut wide_code = CpuState::protected_mode(Bits32, Bits16);
    wide_code.segments.cs.big = 0x80; // Every nonzero cache byte represents CS.D=1.
    wide_code.flags.set_eflags(0x3002); // The physical PC can enter with IF clear and IOPL 3.
    let pc = CpuConfig::pc(CpuState::real_mode()).unwrap();
    for protected in [CpuState::protected_mode(Bits16, Bits32), wide_code] {
        assert!(CpuConfig::pc(protected).is_ok());
        assert!(matches!(
            CpuConfig::new(pc.compiler_config(), protected),
            Err(ConfigurationError::IncompatibleCpu)
        ));
    }
    // Setting PE on the reset real-mode image leaves CS at ring zero.
    let mut cpu = CpuState::real_mode();
    cpu.cr0 |= 1;
    assert!(matches!(
        CpuConfig::pc(cpu),
        Err(ConfigurationError::IncompatibleCpu)
    ));
}

async fn check_pc_configuration() {
    let config = MachineConfig::new(0, PhysicalMemoryMap::new([]).unwrap()).unwrap();
    let memory = Memory::new(config).unwrap();
    for (entry, backend, state, expected) in [
        (
            EntryAssumptions::FLAT32,
            MemoryBackend::PhysicalBus,
            CpuState::default(),
            ConfigurationError::UnsupportedEntryAssumptions,
        ),
        (
            // The interpreter can change IF before it dispatches to compiled code.
            EntryAssumptions {
                interrupt_flag: Some(false),
                ..EntryAssumptions::REAL_MODE
            },
            MemoryBackend::PhysicalBus,
            CpuState::real_mode(),
            ConfigurationError::UnsupportedEntryAssumptions,
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
    let fixed = Cpu::new(memory, real, Default::default()).await.unwrap();
    assert_eq!(
        fixed.cpu_state().restore(&CpuState::default()),
        Err(ConfigurationError::IncompatibleCpu)
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn physical_pc_checks_compiler_entry_and_memory_backend() {
    pollster::block_on(check_pc_configuration());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn physical_pc_checks_compiler_entry_and_memory_backend() {
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
