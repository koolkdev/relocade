use relocade_machine::{CpuConfig, MachineConfig, MemoryError, PagedMemoryMap};
use relocade_machine::{Machine, RuntimeState};
use relocade_wasm_runtime::{Backend, WasmBackend};
use relocade_x86::{Compiler, CpuState, Exception, SegmentProfile};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

fn backend() -> Backend<RuntimeState> {
    Backend::with_state(RuntimeState::default()).unwrap()
}

fn config() -> MachineConfig {
    let mut map = PagedMemoryMap::default();
    map.map(0x4000..=0x4fff, 0, true).unwrap();
    map.map(0x5000..=0x5fff, 0x1000, false).unwrap();
    MachineConfig::new(0x3000, map).unwrap()
}

fn check_live_paging() {
    let mut first = backend();
    let space = Machine::new(&mut first, config()).unwrap();
    let cpu = CpuConfig::new(
        Compiler::new(SegmentProfile::Flat32).target(),
        CpuState::default(),
    )
    .unwrap();
    let memory = space.bind_cpu(&mut first, cpu).unwrap();
    let mut second = backend();
    let peer = space.bind_cpu(&mut second, cpu).unwrap();
    memory.with_host_call(&mut first.context(), |call| {
        call.memory().write_backing(0, b"first").unwrap();
        call.memory().write_backing(0x2000, b"other").unwrap();
    });
    let mut independent = space.memory();
    let reader = independent.view();
    assert_eq!(reader.read_u8(0x4000).unwrap(), b'f');
    memory.with_host_call(&mut first.context(), |call| {
        call.memory().write_u8(0x4000, b'F').unwrap()
    });
    assert_eq!(reader.read_u8(0x4000).unwrap(), b'F');
    space
        .memory()
        .view()
        .map_pages(0x4000..=0x4fff, 0x2000, true)
        .unwrap();
    memory.with_host_call(&mut first.context(), |call| {
        assert_eq!(call.memory().read_u8(0x4000).unwrap(), b'o')
    });
    assert_eq!(reader.read_u8(0x4000).unwrap(), b'o');
    peer.with_host_call(&mut second.context(), |call| {
        call.memory().unmap_pages(0x5000..=0x5fff).unwrap()
    });
    let mut bytes = [0x55; 4];
    assert_eq!(
        reader.read(0x4ffe, &mut bytes),
        Err(MemoryError::PageFault(Exception::PageFault {
            linear_address: 0x5000,
            error_code: 0,
        }))
    );
    assert_eq!(bytes, [0x55; 4]);
    // CPU resources are not needed to preserve the live map or its backing.
    drop(first);
    drop(second);
    assert_eq!(reader.read_u8(0x4000).unwrap(), b'o');
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn cpus_and_readers_share_live_page_tables() {
    check_live_paging();
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
fn cpus_and_readers_share_live_page_tables() {
    check_live_paging();
}
