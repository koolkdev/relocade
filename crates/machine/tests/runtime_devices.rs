use relocade_machine::{Cpu, CpuConfig, Hook, HostAction, IoBindings, MachineConfig, Memory};
use relocade_wasm_runtime::{Backend, SharedMemory, WasmContext};
use relocade_x86::{
    Compiler, CpuState, ExecutionExit, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap,
    Segments,
};
use std::{cell::RefCell, rc::Rc};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[derive(Default)]
struct Device {
    value: u16,
    port: u16,
    mmio_reads: u32,
    port_reads: u32,
    memory: Option<Memory<SharedMemory>>,
}

async fn check_device_access() {
    let device = Rc::new(RefCell::new(Device::default()));
    let backend = Backend::with_state(Rc::clone(&device)).unwrap();
    let io = IoBindings::<Backend<Rc<RefCell<Device>>>> {
        read_port: Box::new(|caller, port, width| {
            assert_eq!((port, width), (0x1234, 2));
            let mut device = caller.state().borrow_mut();
            device.port_reads += 1;
            i32::from(device.port)
        }),
        write_port: Box::new(|caller, port, width, value| {
            assert_eq!((port, width, value), (0x1234, 2, 7));
            caller.state().borrow_mut().port = value as u16 + 1;
        }),
        read_mmio: Box::new(|caller, address, width| {
            assert_eq!((address, width), (0x8000, 2));
            let mut device = caller.state().borrow_mut();
            device.mmio_reads += 1;
            i64::from(device.value)
        }),
        write_mmio: Box::new(|caller, address, width, value| {
            assert_eq!((address, width, value), (0x8000, 2, 7));
            let device = Rc::clone(caller.state());
            let mut device = device.borrow_mut();
            device.value = value as u16;
            // DMA uses the same RAM owner retained by the embedding.
            device
                .memory
                .as_mut()
                .unwrap()
                .view()
                .write_u16(0x500, 70)
                .unwrap();
        }),
    };
    let config = MachineConfig::new(
        0x10000,
        PhysicalMemoryMap::new([
            (0..=0x7fff, PhysicalMapping::Ram { backing_offset: 0 }),
            (0x8000..=0x8fff, PhysicalMapping::Mmio),
            (
                0x9000..=0xffff,
                PhysicalMapping::Ram {
                    backing_offset: 0x9000,
                },
            ),
        ])
        .unwrap(),
    )
    .unwrap();
    let cpu = CpuConfig::new(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            eip: 0x100,
            segments: Segments::real_mode(),
            ..CpuState::default()
        },
    )
    .unwrap();
    let mut runtime = Cpu::with_io(backend, config, cpu, io).await.unwrap();
    device.borrow_mut().memory = Some(runtime.memory());
    runtime
        .context()
        .memory()
        .write(
            0x100,
            &[
                0xb8, 7, 0, // MOV AX,7
                0xa3, 0, 0x80, // MOV [8000],AX (MMIO)
                0xa1, 0, 0x80, // MOV AX,[8000]
                0xba, 0x34, 0x12, // MOV DX,1234
                0xef, // OUT DX,AX
                0xed, // IN AX,DX
                0xe9, 0x0f, 0, // JMP 0120 (host hook)
            ],
        )
        .unwrap();
    runtime
        .context()
        .memory()
        .write(
            0x140,
            &[
                0xa1, 0, 0x80, // MOV AX,[8000]
                0xa3, 0, 3,    // MOV [0300],AX
                0xed, // IN AX,DX
                0xe9, 6, 0, // JMP 0150 (host return)
            ],
        )
        .unwrap();
    let hook_device = Rc::clone(&device);
    let mut calls = 0;
    runtime
        .install_hooks([
            Hook::new(0x120, move |context| {
                calls += 1;
                assert_eq!(calls, 1);
                assert_eq!(context.cpu().ax(), 8);
                assert_eq!(context.memory().read_u16(0x500).unwrap(), 70);
                let mut device = hook_device.borrow_mut();
                assert_eq!((device.value, device.port), (7, 8));
                device.value = 12;
                device.port = 13;
                context.memory().write_u16(0x502, 120).unwrap();
                context.cpu().set_eip(0x140);
                HostAction::Continue
            }),
            Hook::new(0x150, |_| HostAction::Exit(ExecutionExit::HostReturn)),
        ])
        .await
        .unwrap();
    assert_eq!(runtime.run(100).unwrap(), ExecutionExit::HostReturn);
    assert_eq!(runtime.context().cpu().ax(), 13);
    assert_eq!(runtime.context().cpu().instruction_count(), 11);
    assert_eq!(runtime.context().memory().read_u16(0x300).unwrap(), 12);
    let mut memory = runtime.memory();
    assert_eq!(memory.view().read_u16(0x502).unwrap(), 120);
    let device = device.borrow();
    assert_eq!((device.mmio_reads, device.port_reads), (2, 2));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn inline_devices_and_hooks_share_owned_state_and_guest_ram() {
    pollster::block_on(check_device_access());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn inline_devices_and_hooks_share_owned_state_and_guest_ram() {
    check_device_access().await;
}
