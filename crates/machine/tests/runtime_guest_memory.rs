use relocade_machine::{Cpu, RuntimeState};
use relocade_machine::{MachineConfig, MemoryError, ReadMemory};
use relocade_x86::{
    Compiler, CpuState, ExecutionExit, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap,
    Segments,
};

use relocade_wasm_runtime::Backend;
#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

async fn check_guest_memory() {
    let mut map = PhysicalMemoryMap::new([
        (
            0..=0xfff,
            PhysicalMapping::Ram {
                backing_offset: 0x2000,
            },
        ),
        (
            0x1000..=0x1fff,
            PhysicalMapping::Rom {
                backing_offset: 0x1000,
            },
        ),
        (0x3000..=0x3fff, PhysicalMapping::Mmio),
    ])
    .unwrap();
    map.set_a20_enabled(false);
    let config = MachineConfig::new(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            eip: 0x100,
            segments: Segments::real_mode(),
            ..CpuState::default()
        },
        0x3000,
        map,
    )
    .unwrap();
    let backend = Backend::with_state(RuntimeState::new(())).unwrap();
    let mut runtime = Cpu::new(backend, config).await.unwrap();
    let reader = runtime.shared_memory();
    runtime
        .context()
        .memory()
        .write_backing(0x1000, &[0x12, 0x34])
        .unwrap();
    // MOV word [0FFE],0741; JMP to the next instruction.
    runtime
        .context()
        .memory()
        .write(0x100, &[0xc7, 0x06, 0xfe, 0x0f, 0x41, 0x07, 0xeb, 0])
        .unwrap();
    assert_eq!(runtime.run(1).unwrap(), ExecutionExit::BudgetExhausted);
    runtime.context().memory().write(0x1000, &[0, 0]).unwrap(); // ROM ignores writes.
    let mut bytes = [0; 4];
    reader.read(0xffe, &mut bytes).unwrap();
    assert_eq!(bytes, [0x41, 7, 0x12, 0x34]);
    reader.read(0x100ffe, &mut bytes).unwrap(); // A20 alias.
    assert_eq!(bytes, [0x41, 7, 0x12, 0x34]);
    for hole in [0x2000, 0x3000, 0x80000000] {
        reader.read(hole, &mut bytes).unwrap();
        assert_eq!(bytes, [0xff; 4]);
    }
    assert_eq!(
        reader.read(u32::MAX, &mut bytes),
        Err(MemoryError::AddressRange)
    );
    assert_eq!(
        reader.read_backing(0x3000, &mut bytes),
        Err(MemoryError::BackingRange)
    );
    drop(runtime);
    assert_eq!(reader.read_u16(0xffe).unwrap(), 0x0741);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn shared_guest_memory_uses_the_machine_map() {
    pollster::block_on(check_guest_memory());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn shared_guest_memory_uses_the_machine_map() {
    check_guest_memory().await;
}
