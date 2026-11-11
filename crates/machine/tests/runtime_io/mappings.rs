use super::*;
use relocade_machine::{Hook, PortRead, PortWrite};
use relocade_x86::HostExit;

macro_rules! test_backends {
    ($name:ident, $check:ident) => {
        #[cfg(not(target_arch = "wasm32"))]
        #[test]
        fn $name() {
            pollster::block_on($check());
        }
        #[cfg(target_arch = "wasm32")]
        #[wasm_bindgen_test::wasm_bindgen_test]
        async fn $name() {
            $check().await;
        }
    };
}

#[path = "mappings/configuration.rs"]
mod configuration;

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

async fn check_late_connections() {
    for compiled in [false, true] {
        let io = IoBus::default();
        // Link both port directions while the bus is still unconnected.
        let mut cpu = guest(io.clone(), &[0x66, 0xef, 0x66, 0xed], &[0, 2], compiled).await;
        cpu.cpu_state().set_dx(0xffff);
        run(&mut cpu).await;
        assert_eq!(cpu.cpu_state().eax(), u32::MAX);

        let device = RecordingDevice::new(0);
        cpu.io().connect([DeviceConnection {
            device: device.clone(),
            ports: std::iter::once(0xffff..=0xffff).collect(),
            mmio: vec![],
        }]);
        cpu.cpu_state().set_eip(0x100);
        cpu.cpu_state().set_eax(0x4433_2211);
        run(&mut cpu).await;
        assert_eq!(cpu.cpu_state().eax(), 0x4433_2211);
        assert_eq!(
            device.borrow_mut().take_transfers(),
            [
                Transfer::PortWrite {
                    port: 0xffff,
                    bytes: 4,
                    value: 0x4433_2211
                },
                Transfer::PortRead {
                    port: 0xffff,
                    bytes: 4
                },
            ]
        );
        assert_eq!(io.read_port(0xffff, 4), 0x4433_2211);
    }
}

test_backends!(
    initial_connections_update_already_linked_port_tables,
    check_late_connections
);

async fn check_shared_registrations() {
    for compiled in [false, true] {
        let device = RecordingDevice::new(0);
        device.borrow_mut().port = 0x1234_5678;
        let io = super::ports::port_bus(&device);
        let ports = io.ports(&device);
        let mut cpu = guest(io.clone(), &[0x66, 0xed], &[0], compiled).await;
        read(&mut cpu, 0x1234_5678).await;
        let reusable = PortRead::new(|_, _, _| 17);
        io.with_context(|context| ports.set_read(context, 0xfffe..=0xffff, &reusable));
        assert_eq!(io.read_port(0xffff, 4), 17);
        read(&mut cpu, 17).await;
        let other = PortRead::new(|_, _, _| 29);
        io.with_context(|context| ports.set_read(context, 0xffff, &other));
        read(&mut cpu, 29).await;
        assert_eq!(io.read_port(0xfffe, 4), 17);
        io.with_context(|context| ports.set_read(context, 0xffff, &reusable));
        read(&mut cpu, 17).await;
        io.with_context(|context| ports.remove_read(context, 0xffff));
        assert_eq!(io.read_port(0xffff, 4), u32::MAX);
        read(&mut cpu, u32::MAX).await;
        // Removing reads does not uncover the legacy handler or remove writes.
        io.write_port(0xffff, 4, 0x7654_3210);
        assert_eq!(device.borrow().port, 0x7654_3210);

        let started = Rc::new(std::cell::Cell::new(0));
        let captured = started.clone();
        let start_read = PortRead::new(move |_, port, bytes| {
            assert_eq!((port, bytes), (0x3d5, 1));
            captured.set(captured.get() + 1);
            0
        });
        io.with_context(|context| ports.set_read(context, 0x3d5, &start_read));
        cpu.install_module(START_READ, &[]).await.unwrap();
        assert_eq!(started.get(), 1);
    }
}

async fn check_individual_handlers_and_removal() {
    for compiled in [false, true] {
        let io = IoBus::default();
        let device = RecordingDevice::new(0);
        let ports = io.ports(&device);
        let writes = Rc::new(RefCell::new(Vec::new()));
        let captured = writes.clone();
        {
            let index = PortRead::new(|_, _, _| 17);
            let data = PortRead::new(|_, _, _| 29);
            let status = PortRead::new(|_, _, _| 9);
            let write = PortWrite::new(move |_, port, bytes, value| {
                captured.borrow_mut().push((port, bytes, value))
            });
            io.with_context(|context| ports.set_read(context, 0x3d4, &index));
            io.with_context(|context| ports.set_read(context, 0x3d5, &data));
            io.with_context(|context| ports.set_read(context, 0x3da, &status));
            io.with_context(|context| ports.set_write(context, 0x3d4, &write));
        }
        let mut cpu = guest(io.clone(), &[0xec, 0x42, 0xec, 0xee], &[0], compiled).await;
        assert!(!io.is_empty());
        cpu.cpu_state().set_dx(0x3d4);
        run(&mut cpu).await;
        assert_eq!(cpu.cpu_state().al(), 29);
        assert!(writes.borrow().is_empty()); // 3D5 has no write handler.
        io.write_port(0x3d4, 2, 0x1234_5678);
        assert_eq!(&*writes.borrow(), &[(0x3d4, 2, 0x5678)]);
        assert_eq!(io.read_port(0x3da, 1), 9);
        let status = PortRead::new(|_, _, _| 1);
        io.with_context(|context| ports.set_read(context, 0x3da, &status));
        assert_eq!(io.read_port(0x3d5, 1), 29);
        io.with_context(|context| ports.remove_write(context, 0x3d4));
        io.write_port(0x3d4, 1, 7);
        assert_eq!(writes.borrow().len(), 1);
        let owner = ports.clone();
        drop(ports);
        assert_eq!(io.read_port(0x3d5, 1), 29);
        // Dropping a handle leaves registrations installed.
        io.with_context(|context| owner.clear(context));
        drop(owner);
        assert!(io.is_empty());
        assert_eq!(io.read_port(0x3d5, 1), 0xff);
        cpu.cpu_state().set_eip(0x100);
        cpu.cpu_state().set_dx(0x3d4);
        run(&mut cpu).await;
        assert_eq!(cpu.cpu_state().al(), 0xff);
        let replacement_device = RecordingDevice::new(0);
        let replacement = io.ports(&replacement_device);
        let data = PortRead::new(|_, _, _| 41);
        io.with_context(|context| replacement.set_read(context, 0x3d5, &data));
        cpu.cpu_state().set_eip(0x100);
        cpu.cpu_state().set_dx(0x3d4);
        run(&mut cpu).await;
        assert_eq!(cpu.cpu_state().al(), 41);
        io.with_context(|context| replacement.clear(context));
        assert!(io.is_empty());
        let persistent_device = RecordingDevice::new(0);
        let persistent = io.ports(&persistent_device);
        let data = PortRead::new(|_, _, _| 53);
        io.with_context(|context| persistent.set_read(context, 0x3d5, &data));
        drop(persistent);
        assert_eq!(io.read_port(0x3d5, 1), 53);
        cpu.cpu_state().set_eip(0x100);
        cpu.cpu_state().set_dx(0x3d4);
        run(&mut cpu).await;
        assert_eq!(cpu.cpu_state().al(), 53);
    }
}

async fn check_widths() {
    for compiled in [false, true] {
        let io = IoBus::default();
        let device = RecordingDevice::new(0);
        let ports = io.ports(&device);
        let mut cpu = guest(
            io.clone(),
            &[0xec, 0xed, 0x66, 0xed, 0xee, 0xef, 0x66, 0xef],
            &[0, 1, 2, 4, 5, 6],
            compiled,
        )
        .await;
        let reads = Rc::new(RefCell::new(Vec::new()));
        let captured = reads.clone();
        let read = PortRead::new(move |_, port, bytes| {
            captured.borrow_mut().push((port, bytes));
            0x8765_4321
        });
        let writes = Rc::new(RefCell::new(Vec::new()));
        let captured = writes.clone();
        let write = PortWrite::new(move |_, port, bytes, value| {
            captured.borrow_mut().push((port, bytes, value))
        });
        io.with_context(|context| ports.set_read(context, 0xffff, &read));
        io.with_context(|context| ports.set_write(context, 0xffff, &write));
        cpu.cpu_state().set_dx(0xffff);
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
        io.with_context(|context| ports.clear(context));
        cpu.cpu_state().set_eip(0x100);
        run(&mut cpu).await;
        assert_eq!(cpu.cpu_state().eax(), u32::MAX);
        assert_eq!(reads.borrow().len(), 3);
        assert_eq!(writes.borrow().len(), 3);
    }
}

test_backends!(
    port_registrations_are_shared_by_host_and_cpu,
    check_shared_registrations
);

#[cfg(not(target_arch = "wasm32"))]
#[test]
#[should_panic(expected = "port handler is bound to another I/O bus")]
fn prepared_handlers_cannot_move_to_another_cpu() {
    pollster::block_on(async {
        let io = IoBus::default();
        let device = RecordingDevice::new(0);
        let ports = io.ports(&device);
        let _cpu = guest(io.clone(), &[0xec], &[0], false).await;
        let read = PortRead::new(|_, _, _| 9);
        io.with_context(|context| ports.set_read(context, 0x80, &read));
        let io = IoBus::default();
        let device = RecordingDevice::new(0);
        let ports = io.ports(&device);
        let _other = guest(io.clone(), &[0xec], &[0], false).await;
        io.with_context(|context| ports.set_read(context, 0x80, &read));
    });
}

test_backends!(
    devices_edit_independent_port_entries_and_remove_them_explicitly,
    check_individual_handlers_and_removal
);

test_backends!(registered_ports_preserve_architectural_widths, check_widths);

async fn check_disjoint_connections() {
    let device = RecordingDevice::new(0);
    let erased: Rc<RefCell<dyn Device>> = device.clone();
    let io = IoBus::new([0x80, 0x81].map(|port| DeviceConnection {
        device: device.clone(),
        mmio: vec![],
        ports: std::iter::once(port..=port).collect(),
    }));
    let ports = io.ports(&device);
    let _cpu = guest(io.clone(), &[0xec], &[0], false).await;
    let first = PortRead::new(|_, _, _| 17);
    let second = PortRead::new(|_, _, _| 29);
    io.with_context(|context| ports.set_read(context, 0x80, &first));
    io.with_context(|context| io.ports(&erased).set_read(context, 0x81, &second));
    assert_eq!(io.read_port(0x80, 1), 17);
    assert_eq!(io.read_port(0x81, 1), 29);
    drop(ports);
    io.with_context(|context| io.ports(&device).clear(context));
    assert_eq!(io.read_port(0x80, 1), 0xff);
    assert_eq!(io.read_port(0x81, 1), 0xff);
    io.write_port(0x81, 1, 7);
    assert!(device.borrow().transfers.is_empty());
}

test_backends!(
    disjoint_connections_for_one_device_share_registration_ownership,
    check_disjoint_connections
);

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn unconnected_devices_share_handles_without_being_retained() {
    let io = IoBus::default();
    let device = RecordingDevice::new(0);
    let weak = Rc::downgrade(&device);
    let ports = io.ports(&device);
    assert!(io.is_empty());
    io.with_context(|context| ports.set_read(context, 0x80, &PortRead::new(|_, _, _| 17)));
    drop(ports);
    let ports = io.ports(&device);
    io.with_context(|context| ports.set_read(context, 0x80, &PortRead::new(|_, _, _| 29)));
    drop(device);
    assert!(weak.upgrade().is_none());
    assert_eq!(io.read_port(0x80, 1), 29);
    io.with_context(|context| ports.clear(context));
    assert!(io.is_empty());
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

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn context_from_another_bus_cannot_edit_registrations() {
    use std::panic::{catch_unwind, AssertUnwindSafe};
    let io = IoBus::default();
    let device = RecordingDevice::new(0);
    let ports = io.ports(&device);
    io.with_context(|context| ports.set_read(context, 0x80, &PortRead::new(|_, _, _| 17)));
    let foreign = IoBus::default();
    assert!(catch_unwind(AssertUnwindSafe(|| {
        foreign.with_context(|context| {
            ports.set_read(context, 0x80, &PortRead::new(|_, _, _| 29));
        });
    }))
    .is_err());
    assert_eq!(io.read_port(0x80, 1), 17);
    assert_eq!(foreign.read_port(0x80, 1), 0xff);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn conflicting_ranges_leave_host_and_guest_registrations_unchanged() {
    use std::panic::{catch_unwind, AssertUnwindSafe};
    pollster::block_on(async {
        let io = IoBus::default();
        let first_device = RecordingDevice::new(0);
        let other_device = RecordingDevice::new(0);
        let first = io.ports(&first_device);
        let other = io.ports(&other_device);
        io.with_context(|context| first.set_read(context, 0x80, &PortRead::new(|_, _, _| 17)));
        io.with_context(|context| other.set_read(context, 0x82, &PortRead::new(|_, _, _| 29)));
        let mut cpu = guest(io.clone(), &[0xec], &[0], true).await;
        assert!(catch_unwind(AssertUnwindSafe(|| io.with_context(
            |context| first.set_read(context, 0x80..=0x82, &PortRead::new(|_, _, _| 41))
        )))
        .is_err());
        assert_eq!(
            [
                io.read_port(0x80, 1),
                io.read_port(0x81, 1),
                io.read_port(0x82, 1)
            ],
            [17, 0xff, 29]
        );
        cpu.cpu_state().set_dx(0x80);
        run(&mut cpu).await;
        assert_eq!(cpu.cpu_state().al(), 17);

        let bound = PortRead::new(|_, _, _| 53);
        io.with_context(|context| first.set_read(context, 0x80, &bound));
        let new_bus = IoBus::default();
        let unbound = new_bus.ports(&first_device);
        assert!(catch_unwind(AssertUnwindSafe(
            || new_bus.with_context(|context| unbound.set_read(context, 0x80, &bound))
        ))
        .is_err());
        assert!(new_bus.is_empty());
    });
}
