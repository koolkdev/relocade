mod support;
use relocade_machine::{Cpu, Memory, MemoryError};
use relocade_x86::{
    Compiler, CpuState, ExecutionExit, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap,
    Segments,
};

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
    let config = support::configuration(
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
    let shared = Memory::new(config.machine).unwrap();
    let reader = shared.view();
    shared.view().write_backing(0x1000, &[0x12, 0x34]).unwrap();
    // MOV word [0FFE],0741; JMP to the next instruction.
    shared
        .view()
        .write(0x100, &[0xc7, 0x06, 0xfe, 0x0f, 0x41, 0x07, 0xeb, 0])
        .unwrap();
    let mut runtime = Cpu::new(shared.clone(), config.cpu, Default::default())
        .await
        .unwrap();
    let mut other = Cpu::new(shared.clone(), config.cpu, Default::default())
        .await
        .unwrap();
    runtime.context().cpu.set_ax(0x1234);
    assert_eq!(runtime.run(1).await, ExecutionExit::BudgetExhausted);
    assert_eq!(runtime.context().cpu.eip(), 0x108);
    assert_eq!(other.context().cpu.eip(), 0x100);
    assert_eq!(other.context().cpu.ax(), 0);
    assert_eq!(other.memory().view().read_u16(0xffe).unwrap(), 0x0741);
    runtime.context().memory.write(0x1000, &[0, 0]).unwrap(); // ROM ignores writes.
    let mut bytes = [0; 4];
    reader.read(0xffe, &mut bytes).unwrap();
    assert_eq!(bytes, [0x41, 7, 0x12, 0x34]);
    reader.read(0x100ffe, &mut bytes).unwrap(); // A20 alias.
    assert_eq!(bytes, [0x41, 7, 0x12, 0x34]);
    for hole in [0x2000, 0x80000000] {
        reader.read(hole, &mut bytes).unwrap();
        assert_eq!(bytes, [0xff; 4]);
    }
    runtime.context().memory.read(0x3000, &mut bytes).unwrap();
    assert_eq!(bytes, [0xff; 4]);
    assert_eq!(
        reader.read(u32::MAX, &mut bytes),
        Err(MemoryError::AddressRange)
    );
    assert_eq!(
        reader.read_backing(0x3000, &mut bytes),
        Err(MemoryError::BackingRange)
    );
    drop(runtime);
    drop(other);
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
