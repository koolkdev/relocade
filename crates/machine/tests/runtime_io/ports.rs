use super::*;
use relocade_machine::{Hook, PortRead, PortWrite};
use relocade_x86::HostExit;

async fn guest(io: IoBus, code: &[u8], entries: &[usize], compiled: bool) -> Cpu {
    let compiler = Compiler::new(ExecutionProfile::Real16);
    let config = support::configuration(
        compiler.target(),
        CpuState {
            eip: 0x100,
            segments: Segments::real_mode(),
            ..Default::default()
        },
        0x5000,
        map(),
    )
    .unwrap();
    let memory = Memory::new(config.machine).unwrap();
    memory.view().write(0x100, code).unwrap();
    let mut cpu = Cpu::new(memory, config.cpu, io).await.unwrap();
    if compiled {
        let snapshots: Vec<_> = entries
            .iter()
            .map(|&offset| BlockSnapshot {
                start_eip: 0x100 + offset as u32,
                bytes: &code[offset..],
                instruction_limit: 16,
            })
            .collect();
        let module = compiler.compile_blocks(&snapshots).unwrap();
        let entries: Vec<_> = module
            .entries
            .iter()
            .map(|entry| DispatchEntry {
                address: entry.start_eip,
                export: &entry.export,
            })
            .collect();
        cpu.install_module(&module.bytes, &entries).await.unwrap();
    }
    let exit = cpu.exit_handle();
    cpu.install([Hook::host(0x100 + code.len() as u32, async move || {
        exit.request()
    })])
    .await
    .unwrap();
    cpu
}

async fn run(cpu: &mut Cpu) {
    assert_eq!(cpu.run().await, ExecutionExit::Host(HostExit::Return));
}

async fn read(cpu: &mut Cpu, expected: u32) {
    cpu.context().cpu.set_eip(0x100);
    cpu.context().cpu.set_edx(0xabcd_ffff);
    run(cpu).await;
    assert_eq!(cpu.context().cpu.eax(), expected);
}

async fn check_local_updates() {
    for compiled in [false, true] {
        let device = RecordingDevice::new(0x3000);
        device.borrow_mut().port = 0x1234_5678;
        let io = IoBus::new([DeviceConnection {
            device: device.clone(),
            mmio: vec![],
            ports: std::iter::once(0xffff..=0xffff).collect(),
        }]);
        let mut first = guest(io.clone(), &[0x66, 0xed], &[0], compiled).await;
        let mut second = guest(io.clone(), &[0x66, 0xed], &[0], !compiled).await;
        first
            .port_mappings()
            .set_read(0xfffe..=0xffff, Some(PortRead::new(|_, _| 17)));
        read(&mut first, 17).await;
        read(&mut second, 0x1234_5678).await;
        second
            .port_mappings()
            .set_read(0xffff..=0xffff, Some(PortRead::new(|_, _| 23)));
        let resource = Rc::new(29);
        let lifetime = Rc::downgrade(&resource);
        first
            .port_mappings()
            .set_read(0xffff..=0xffff, Some(PortRead::new(move |_, _| *resource)));
        read(&mut first, 29).await;
        read(&mut second, 23).await;
        assert_eq!(io.read_port(0xffff, 4), 0x1234_5678);
        first.context().cpu.set_eip(0x100);
        first.context().cpu.set_dx(0xfffe);
        run(&mut first).await;
        assert_eq!(first.context().cpu.eax(), 17);
        first.context().cpu.set_dx(0xffff);
        first
            .port_mappings()
            .set_read(0xffff..=0xffff, Some(PortRead::open_bus()));
        assert!(lifetime.upgrade().is_none());
        first.context().cpu.set_eip(0x100);
        assert_eq!(first.step(), ExecutionExit::StepComplete);
        assert_eq!(first.context().cpu.eax(), u32::MAX);
        first.port_mappings().set_read(0xffff..=0xffff, None);
        read(&mut first, 0x1234_5678).await;
        read(&mut second, 23).await;

        // Instantiation may itself perform I/O from a module start function.
        let started = Rc::new(std::cell::Cell::new(0));
        let captured = started.clone();
        first.port_mappings().set_read(
            0x3d5..=0x3d5,
            Some(PortRead::new(move |port, bytes| {
                assert_eq!((port, bytes), (0x3d5, 1));
                captured.set(captured.get() + 1);
                0
            })),
        );
        first.install_module(START_READ, &[]).await.unwrap();
        assert_eq!(started.get(), 1);
        second.install_module(START_READ, &[]).await.unwrap();
        assert_eq!(started.get(), 1);

        let resource = Rc::new(31);
        let lifetime = Rc::downgrade(&resource);
        first
            .port_mappings()
            .set_read(0xffff..=0xffff, Some(PortRead::new(move |_, _| *resource)));
        drop(first);
        assert!(lifetime.upgrade().is_none());
        read(&mut second, 23).await;
        assert_eq!(io.read_port(0xffff, 1), 0x78);
    }
}

async fn check_widths_and_restoration() {
    for compiled in [false, true] {
        let device = RecordingDevice::new(0);
        device.borrow_mut().port = 0x1234_5678;
        let io = IoBus::new([DeviceConnection {
            device: device.clone(),
            mmio: vec![],
            ports: std::iter::once(0xffff..=0xffff).collect(),
        }]);
        let mut cpu = guest(
            io,
            &[0xec, 0xed, 0x66, 0xed, 0xee, 0xef, 0x66, 0xef],
            &[0, 1, 2, 4, 5, 6],
            compiled,
        )
        .await;
        let reads = Rc::new(RefCell::new(Vec::new()));
        let captured = reads.clone();
        cpu.port_mappings().set_read(
            0xffff..=0xffff,
            Some(PortRead::new(move |port, bytes| {
                captured.borrow_mut().push((port, bytes));
                0x8765_4321
            })),
        );
        let writes = Rc::new(RefCell::new(Vec::new()));
        let captured = writes.clone();
        cpu.port_mappings().set_write(
            0xffff..=0xffff,
            Some(PortWrite::new(move |port, bytes, value| {
                captured.borrow_mut().push((port, bytes, value));
            })),
        );
        cpu.context().cpu.set_dx(0xffff);
        run(&mut cpu).await;
        assert_eq!(&*reads.borrow(), &[(0xffff, 1), (0xffff, 2), (0xffff, 4)]);
        assert_eq!(
            &*writes.borrow(),
            &[
                (0xffff, 1, 0x21),
                (0xffff, 2, 0x4321),
                (0xffff, 4, 0x8765_4321)
            ]
        );
        assert!(device.borrow().transfers.is_empty());
        cpu.port_mappings()
            .set_read(0xffff..=0xffff, Some(PortRead::open_bus()));
        cpu.port_mappings()
            .set_write(0xffff..=0xffff, Some(PortWrite::ignored()));
        cpu.context().cpu.set_eip(0x100);
        run(&mut cpu).await;
        assert_eq!(cpu.context().cpu.eax(), u32::MAX);
        assert!(device.borrow().transfers.is_empty());
        assert_eq!(writes.borrow().len(), 3);
        cpu.port_mappings().set_read(0xffff..=0xffff, None);
        cpu.port_mappings().set_write(0xffff..=0xffff, None);
        cpu.context().cpu.set_eip(0x100);
        run(&mut cpu).await;
        assert_eq!(cpu.context().cpu.eax(), 0x1234_5678);
        assert_eq!(device.borrow().port, 0x1234_5678);
        assert_eq!(
            device.borrow_mut().take_transfers(),
            [
                Transfer::PortRead {
                    port: 0xffff,
                    bytes: 1
                },
                Transfer::PortRead {
                    port: 0xffff,
                    bytes: 2
                },
                Transfer::PortRead {
                    port: 0xffff,
                    bytes: 4
                },
                Transfer::PortWrite {
                    port: 0xffff,
                    bytes: 1,
                    value: 0x78
                },
                Transfer::PortWrite {
                    port: 0xffff,
                    bytes: 2,
                    value: 0x5678
                },
                Transfer::PortWrite {
                    port: 0xffff,
                    bytes: 4,
                    value: 0x1234_5678
                },
            ]
        );
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn cpu_local_mappings_update_existing_code_and_preserve_other_routes() {
    pollster::block_on(check_local_updates());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn cpu_local_mappings_update_existing_code_and_preserve_other_routes() {
    check_local_updates().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn bound_operations_preserve_widths_and_restore_generic_routes() {
    pollster::block_on(check_widths_and_restoration());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn bound_operations_preserve_widths_and_restore_generic_routes() {
    check_widths_and_restoration().await;
}

// Imports portReads and starts a function equivalent to:
// (drop (call_indirect (type (func (param i32 i32) (result i32)))
//   (i32.const 0x3d5) (i32.const 1) (i32.const 0x3d5)))
const START_READ: &[u8] = &[
    0, 97, 115, 109, 1, 0, 0, 0, 1, 10, 2, 96, 2, 127, 127, 1, 127, 96, 0, 0, 2, 29, 1, 8, b'r',
    b'e', b'l', b'o', b'c', b'a', b'd', b'e', 9, b'p', b'o', b'r', b't', b'R', b'e', b'a', b'd',
    b's', 1, 112, 1, 128, 128, 4, 128, 128, 4, 3, 2, 1, 1, 8, 1, 0, 10, 16, 1, 14, 0, 65, 213, 7,
    65, 1, 65, 213, 7, 17, 0, 0, 26, 11,
];
