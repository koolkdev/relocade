use super::*;
use relocade_machine::{IoContext, PortBinding};

async fn check_read_replacement() {
    for compiled in [false, true] {
        let io = IoBus::default();
        let ports = bind(&io, 0xffff);
        let resource = Rc::new(0x4433_2211);
        let next = PortRead::new(|_, _, _| 0x8877_6655);
        let owned_ports = ports.clone();
        let first = PortRead::new(move |context, port, bytes| {
            assert_eq!((port, bytes), (0xffff, 4));
            owned_ports.set_read(context, &next);
            *resource // Self-replacement retains the active invocation's captures.
        });
        io.with_context(|context| ports.set_read(context, &first));
        // The same callback can configure the bus before it has a CPU.
        assert_eq!(io.read_port(0xffff, 4), 0x4433_2211);
        assert_eq!(io.read_port(0xffff, 4), 0x8877_6655);
        let mut cpu = guest(io.clone(), &[0xf3, 0x66, 0x6d], &[0], compiled).await;
        for _ in 0..2 {
            io.with_context(|context| ports.set_read(context, &first));
            cpu.cpu_state().set_eip(0x100);
            cpu.cpu_state().set_dx(0xffff);
            cpu.cpu_state().set_di(0x600);
            cpu.cpu_state().set_cx(3);
            if compiled {
                run(&mut cpu).await;
            } else {
                assert_eq!(cpu.step(), ExecutionExit::StepComplete);
            }
            let mut values = [0; 12];
            cpu.context().memory.read(0x600, &mut values).unwrap();
            assert_eq!(
                values,
                [0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x55, 0x66, 0x77, 0x88]
            );
            assert_eq!(cpu.cpu_state().cx(), 0);
        }
    }
}

async fn check_write_replacement() {
    for compiled in [false, true] {
        let io = IoBus::default();
        let ports = bind(&io, 0x90);
        let mut cpu = guest(io.clone(), &[0xf3, 0x6e], &[0], compiled).await;
        let writes = Rc::new(RefCell::new(Vec::new()));
        let captured = writes.clone();
        let last = PortWrite::new(move |_, port, bytes, value| {
            captured.borrow_mut().push((2, port, bytes, value))
        });
        let captured = writes.clone();
        let owned_ports = ports.clone();
        let first = PortWrite::new(move |context, port, bytes, value| {
            let last = last.clone();
            let next_writes = captured.clone();
            let next_ports = owned_ports.clone();
            // Create a fresh concrete callback while guest execution is suspended.
            owned_ports.set_write(
                context,
                &PortWrite::new(move |context, port, bytes, value| {
                    next_ports.set_write(context, &last);
                    next_writes.borrow_mut().push((1, port, bytes, value));
                }),
            );
            captured.borrow_mut().push((0, port, bytes, value));
        });
        for _ in 0..2 {
            writes.borrow_mut().clear();
            io.with_context(|context| ports.set_write(context, &first));
            cpu.context().memory.write(0x600, &[17, 29, 41]).unwrap();
            cpu.cpu_state().set_eip(0x100);
            cpu.cpu_state().set_dx(0x90);
            cpu.cpu_state().set_si(0x600);
            cpu.cpu_state().set_cx(3);
            run(&mut cpu).await;
            assert_eq!(
                &*writes.borrow(),
                &[(0, 0x90, 1, 17), (1, 0x90, 1, 29), (2, 0x90, 1, 41)]
            );
        }
    }
}

struct ConfigurableDevice {
    ports: Option<PortBinding>,
    status: PortRead,
    idle_read: PortRead,
}

impl ConfigurableDevice {
    fn select_status(&self, context: IoContext<'_>, enabled: bool) {
        let ports = self.ports.as_ref().unwrap();
        let read = if enabled {
            &self.status
        } else {
            &self.idle_read
        };
        ports.set_read(context, read);
    }
}

impl Device for ConfigurableDevice {
    fn write_mmio(&mut self, context: IoContext<'_>, address: u32, bytes: u8, value: u64) {
        assert_eq!((address, bytes), (0x3000, 2));
        self.select_status(context, value & 1 != 0);
    }
    fn read_mmio(&mut self, context: IoContext<'_>, address: u32, bytes: u8) -> u64 {
        assert_eq!((address, bytes), (0x3000, 2));
        self.select_status(context, true);
        0
    }
}

async fn check_device_methods() {
    for compiled in [false, true] {
        for code in [
            &[0xef, 0x42, 0xec][..],
            &[0xa3, 0, 0x30, 0x42, 0xec],
            &[0xa1, 0, 0x30, 0x42, 0xec],
        ] {
            let device = Rc::new(RefCell::new(ConfigurableDevice {
                ports: None,
                status: PortRead::new(|_, _, _| 0x5a),
                idle_read: PortRead::new(|_, _, _| 0x33),
            }));
            let io = IoBus::new(device.clone(), |io| {
                io.map_mmio(std::iter::once(0x3000..=0x3fff));
                let callbacks = relocade_machine::DeviceCallbacks::new(&device);
                io.bind(
                    0x3d4,
                    &device.borrow().idle_read,
                    &PortWrite::word(callbacks.write(|context, device, value| {
                        device.select_status(context, value & 1 != 0);
                    })),
                );
                let status = io.bind(
                    0x3d5,
                    &device.borrow().idle_read,
                    &PortWrite::new(|_, _, _, _| {}),
                );
                device.borrow_mut().ports = Some(status);
                // Initial setup uses the same method as later port/MMIO callbacks.
                device.borrow().select_status(io.context(), true);
                io.port(0x3da)
                    .set_read(io.context(), &PortRead::new(|_, _, _| 9));
            });
            let entries: &[usize] = if code[0] == 0xef { &[0, 1] } else { &[0] };
            let mut cpu = guest(io.clone(), code, entries, compiled).await;
            assert_eq!(io.read_port(0x3d5, 1), 0x5a);
            for value in [1, 0, 1] {
                io.write_port(0x3d4, 2, value ^ 1);
                assert_eq!(io.read_port(0x3d5, 1), if value == 0 { 0x5a } else { 0x33 });
                cpu.cpu_state().set_eip(0x100);
                cpu.cpu_state().set_dx(0x3d4);
                cpu.cpu_state().set_ax(value as u16);
                run(&mut cpu).await;
                let expected = if code[0] == 0xa1 || value != 0 {
                    0x5a
                } else {
                    0x33
                };
                assert_eq!(cpu.cpu_state().al(), expected);
                assert_eq!(io.read_port(0x3d5, 1), u32::from(expected));
                assert_eq!(io.read_port(0x3da, 1), 9);
            }
        }
    }
}

test_backends!(
    reads_replace_themselves_between_rep_elements,
    check_read_replacement
);

test_backends!(
    writes_create_and_replace_callbacks_between_rep_elements,
    check_write_replacement
);

test_backends!(
    ordinary_device_methods_configure_ports_from_host_ports_and_mmio,
    check_device_methods
);
