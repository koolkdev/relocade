use super::*;
use crate::{CpuConfig, Memory};
use relocade_wasm_runtime::{SharedMemory, WasmBackend};
use relocade_x86::{CompilerConfig, CpuState, EntryAssumptions, MemoryBackend, PhysicalMapping};

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn the_last_page_holder_restores_the_configured_write_route() {
    let mut map = PhysicalMemoryMap::new([
        (
            0..=0xfff,
            PhysicalMapping::Ram {
                backing_offset: 0x1000,
            },
        ),
        (0x1000..=0x1fff, PhysicalMapping::Ram { backing_offset: 0 }),
    ])
    .unwrap();
    map.set_code_page(0x1000, true);
    let guest = Memory::new(MachineConfig::new(0x2000, map).unwrap()).unwrap();
    let cpu = CpuConfig::new(
        CompilerConfig::new(EntryAssumptions::REAL_MODE, MemoryBackend::PhysicalBus),
        CpuState::real_mode(),
    )
    .unwrap();
    let binding = guest.attach_cpu(cpu).unwrap();
    let runtime = WasmRuntimeHandle::default();
    let memory = runtime
        .with_runtime(|wasm| RuntimeMemory::new(wasm, binding, SharedMemory::new(1, 1).unwrap()))
        .unwrap();
    let pages = HostWritePages::new(&runtime, &memory);
    let kind =
        |offset| runtime.with_runtime(|wasm| wasm.memory(&memory.mapping).load::<u32>(offset));

    assert_eq!(kind(8), 1); // direct RAM
    let first = pages.hold(0x1000);
    assert_eq!(kind(8), 5); // RAM routed through the host
    let second = pages.hold(0x1080);
    runtime.with_runtime(|wasm| first.release(&mut wasm.context()));
    assert_eq!(kind(8), 5);
    runtime.with_runtime(|wasm| second.release(&mut wasm.context()));
    assert_eq!(kind(8), 1);

    let held_again = pages.hold(0x1000);
    let preconfigured = pages.hold(0);
    drop(preconfigured);
    assert_eq!(kind(0), 5);
    assert_eq!(kind(8), 5);
    drop(held_again);
    assert_eq!(kind(8), 1);
}
