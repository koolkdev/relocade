mod support;
use relocade_machine::{Cpu, FarPointer, MachineConfig, Memory, MemoryError};
use relocade_x86::{
    Compiler, CpuState, ExecutionExit, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap,
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
        (0x3000..=0x3fff, PhysicalMapping::MMIO),
    ])
    .unwrap();
    map.set_a20_enabled(false);
    // Guest stores must reach the tracked-write callback at the backing offset.
    map.set_code_page(0x2000, true);
    let config = support::configuration(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            eip: 0x100,
            ..CpuState::real_mode()
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
    assert_eq!(
        crate::support::checkpoint(&mut runtime).await,
        ExecutionExit::Interrupted
    );
    assert_eq!(runtime.context().cpu.eip(), 0x108);
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

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn far_buffers_wrap_offsets_across_multiple_laps() {
    let map = PhysicalMemoryMap::new([(
        0x12000..=0x22fff,
        PhysicalMapping::Ram { backing_offset: 0 },
    )])
    .unwrap();
    let memory = Memory::new(MachineConfig::new(0x11000, map).unwrap()).unwrap();
    let mut view = memory.view();
    let pointer = FarPointer {
        segment: 0x1200,
        offset: 0xfffe,
    };
    assert_eq!(
        pointer.wrapping_add(0x10003),
        FarPointer {
            segment: 0x1200,
            offset: 1
        }
    );
    view.write(0x22000, &[0xcc; 4]).unwrap();

    let mut input = vec![0x33; 0x10004];
    input[..2].copy_from_slice(b"AB");
    input[0x10000..].copy_from_slice(b"WXYZ");
    view.write_wrapping(pointer, &input).unwrap();
    // The second lap overwrites both ends of the same segment, not its neighbor.
    assert_eq!(view.read_u16(0x21ffe).unwrap(), u16::from_le_bytes(*b"WX"));
    assert_eq!(view.read_u16(0x12000).unwrap(), u16::from_le_bytes(*b"YZ"));
    assert_eq!(view.read_u32(0x22000).unwrap(), 0xcccc_cccc);

    let mut bytes = vec![0; input.len()];
    view.read_wrapping(pointer, &mut bytes).unwrap();
    let mut expected = vec![0x33; 0x10004];
    expected[..4].copy_from_slice(b"WXYZ");
    expected[0x10000..].copy_from_slice(b"WXYZ");
    assert_eq!(bytes, expected);
}

async fn check_flat_guest_memory() {
    use relocade_machine::{CpuConfig, Hook};
    use relocade_x86::{Exception, SegmentProfile};
    const ENTRY: u32 = 0x10_0100;
    const END: u32 = 0x10_1000;

    let memory = Memory::new(MachineConfig::flat(END.into()).unwrap()).unwrap();
    let mut view = memory.view();
    view.write_u32(0x400, 0x11).unwrap();
    view.write_u32(0x10_0400, 0x22).unwrap();
    view.write(
        ENTRY,
        &[
            0xa1, 0x00, 0x04, 0x10, 0x00, // MOV EAX,[100400]
            0xeb, 0x00, // JMP to the host hook
            0x0f, 0x0b, // UD2 if the hook is missed
            0xa1, 0x00, 0x10, 0x10, 0x00, // MOV EAX,[101000] (first unmapped page)
        ],
    )
    .unwrap();
    let config = CpuConfig::new(
        Compiler::new(SegmentProfile::Flat32).target(),
        CpuState {
            eip: ENTRY,
            ..CpuState::default()
        },
    )
    .unwrap();
    let mut cpu = Cpu::new_without_io(memory, config).await.unwrap();
    let state = cpu.cpu_state();
    cpu.install([Hook::host(ENTRY + 7, {
        let state = state.clone();
        async move || state.set_eip(ENTRY + 9)
    })])
    .await
    .unwrap();
    assert_eq!(
        cpu.run().await,
        ExecutionExit::Exception(Exception::PageFault {
            linear_address: END,
            error_code: 0, // Host page table reports an absent page on a read.
        })
    );
    assert_eq!(state.eip(), ENTRY + 9);
    assert_eq!(state.eax(), 0x22);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn flat_cpu_uses_linear_addresses_and_faults_outside_its_memory() {
    pollster::block_on(check_flat_guest_memory());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn flat_cpu_uses_linear_addresses_and_faults_outside_its_memory() {
    check_flat_guest_memory().await;
}

async fn check_shared_flat_page_table() {
    use relocade_machine::CpuConfig;
    use relocade_x86::{Exception, SegmentProfile};

    // Code at a high linear address and data on a separate, aliased page.
    const CODE: u32 = 0xffff_f000;
    const DATA: u32 = 0x4000_0000;
    let memory = Memory::new(MachineConfig::flat(0x3000).unwrap()).unwrap();
    let pages = memory.page_table().unwrap();
    pages.map_pages(CODE..=u32::MAX, 0, true).unwrap();
    let mut view = memory.view();
    view.write(
        CODE,
        &[
            0xa1, 0, 0, 0, 0x40, // MOV EAX,[DATA]
            0xa3, 0, 0, 0, 0x40, // MOV [DATA],EAX
        ],
    )
    .unwrap();
    view.write_u32(0x1000, 31).unwrap();
    view.write_u32(0x2000, 47).unwrap();
    let config = CpuConfig::new(
        Compiler::new(SegmentProfile::Flat32).target(),
        CpuState {
            eip: CODE,
            ..CpuState::default()
        },
    )
    .unwrap();
    let mut first = Cpu::new_without_io(memory.clone(), config).await.unwrap();
    let mut second = Cpu::new_without_io(memory.clone(), config).await.unwrap();

    pages.map_pages(DATA..=DATA + 0xfff, 0x1000, true).unwrap();
    assert_eq!(first.step(), ExecutionExit::StepComplete);
    assert_eq!(first.cpu_state().eax(), 31);
    pages.map_pages(DATA..=DATA + 0xfff, 0x2000, false).unwrap();
    assert_eq!(second.step(), ExecutionExit::StepComplete);
    assert_eq!(second.cpu_state().eax(), 47);
    let fault = Exception::PageFault {
        linear_address: DATA,
        error_code: 3,
    };
    assert_eq!(first.step(), ExecutionExit::Exception(fault));
    assert_eq!(first.cpu_state().eip(), CODE + 5);
    assert_eq!(
        view.write_u32(DATA, 123),
        Err(MemoryError::Translation(fault))
    );
    assert_eq!(view.read_u32(DATA).unwrap(), 47);

    pages.unmap_pages(DATA..=DATA + 0xfff).unwrap();
    let fault = Exception::PageFault {
        linear_address: DATA,
        error_code: 2,
    };
    assert_eq!(second.step(), ExecutionExit::Exception(fault));
    assert_eq!(
        view.write_u32(DATA, 123),
        Err(MemoryError::Translation(fault))
    );
    assert_eq!(view.read_u32(0x2000).unwrap(), 47);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn flat_cpus_and_host_share_live_page_mappings_and_protections() {
    pollster::block_on(check_shared_flat_page_table());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn flat_cpus_and_host_share_live_page_mappings_and_protections() {
    check_shared_flat_page_table().await;
}
