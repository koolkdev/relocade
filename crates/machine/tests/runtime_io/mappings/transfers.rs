//! Prepared operations share transfer behavior across host and guest accesses.

use super::*;
use relocade_machine::DeviceCallbacks;
use std::cell::Cell;

#[derive(Default)]
struct Registers {
    value: u16,
    index: u16,
    tail: Option<PortBinding>,
    tail_write: Option<PortWrite>,
}

impl Device for Registers {}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn exact_widths_reject_access_before_device_borrow() {
    let device = Rc::new(RefCell::new(Registers::default()));
    let mut word = None;
    let tail_value = Rc::new(Cell::new(0));
    let io = IoBus::new(device.clone(), |io| {
        let callbacks = DeviceCallbacks::new(&device);
        io.bind(
            0x80,
            &PortRead::byte(callbacks.read(|_, r| r.value as u8)),
            &PortWrite::byte(callbacks.write(|_, r, value: u8| r.value = value.into())),
        );
        io.bind(
            0x100,
            &PortRead::word(callbacks.read(|_, r| r.index)),
            &PortWrite::word(callbacks.write(|_, r, value| r.index = value)),
        );
        word = Some(io.bind(
            0x101,
            &PortRead::word(callbacks.read(|_, r| r.value)),
            &PortWrite::word(callbacks.write(|_, r, value| r.value = value)),
        ));
        io.bind(
            0xff,
            &PortRead::byte_sequence(0xff..=0x102, |_| 0xff),
            &PortWrite::byte_sequence(0xff..=0x102, |_, _| {}),
        );
        let tail_value = tail_value.clone();
        io.bind(
            0x102,
            &PortRead::byte(|_| 0x5a),
            &PortWrite::byte(move |_, value| tail_value.set(value)),
        );
    });
    io.write_port(0x101, 2, 0x1234);
    io.write_port(0x100, 2, 0x5678);
    assert_eq!(io.read_port(0x100, 2), 0x5678);
    assert_eq!(io.read_port(0x101, 2), 0x1234);
    assert_eq!(io.read_port(0x80, 1), 0x34);
    {
        let registers = device.borrow_mut();
        for (port, bytes, open_bus) in [
            (0x80, 2, 0xffff),
            (0x80, 4, u32::MAX),
            (0x100, 1, 0xff),
            (0x100, 4, u32::MAX),
            (0x101, 1, 0xff),
            (0x101, 4, u32::MAX),
        ] {
            assert_eq!(io.read_port(port, bytes), open_bus);
            io.write_port(port, bytes, 0);
        }
        // Word operations reject byte tails before borrowing the device. The
        // initiating sequence still reaches the byte operation after them.
        assert_eq!(io.read_port(0xff, 4), 0x5affffff);
        io.write_port(0xff, 4, 0x5a123400);
        assert_eq!(tail_value.get(), 0x5a);
        assert_eq!((registers.index, registers.value), (0x5678, 0x1234));
    }
    let word = word.unwrap();
    io.with_context(|context| word.set_read(context, &PortRead::word(|_| 0xabcd)));
    io.write_port(0x101, 2, 0x7856);
    assert_eq!(device.borrow().value, 0x7856);
    assert_eq!(io.read_port(0x101, 2), 0xabcd);
    io.write_port(0x80, 1, 0x42);
    assert_eq!(device.borrow().value, 0x42);
}

async fn check_low_byte_accesses() {
    for compiled in [false, true] {
        let device = Rc::new(RefCell::new(Registers::default()));
        let reads = Rc::new(Cell::new(0));
        let writes = Rc::new(Cell::new(0));
        let io = IoBus::new(device.clone(), |io| {
            let callbacks = DeviceCallbacks::new(&device);
            let reads = reads.clone();
            let writes = writes.clone();
            io.bind(
                0x80,
                &PortRead::low_byte(callbacks.read(move |_, r| {
                    reads.set(reads.get() + 1);
                    r.value as u8
                })),
                &PortWrite::low_byte(callbacks.write(move |_, r, value| {
                    writes.set(writes.get() + 1);
                    r.value = u16::from(value);
                })),
            );
            io.bind(
                0x81,
                &PortRead::byte(|_| panic!("low-byte read continued")),
                &PortWrite::byte(|_, _| panic!("low-byte write continued")),
            );
        });
        // A later mapped address cannot cause an unmapped start to continue.
        assert_eq!(io.read_port(0x7f, 2), 0xffff);
        io.write_port(0x7f, 2, 0x5a00);
        assert_eq!((reads.get(), writes.get()), (0, 0));
        for (index, (bytes, expected)) in [(1, 0x5a), (2, 0xff5a), (4, 0xffffff5a)]
            .into_iter()
            .enumerate()
        {
            io.write_port(0x80, bytes, 0x1234565a);
            assert_eq!(device.borrow().value, 0x5a);
            assert_eq!(io.read_port(0x80, bytes), expected);
            assert_eq!((reads.get(), writes.get()), (index + 1, index + 1));
        }

        let mut cpu = guest(io, &[0x66, 0xef, 0x66, 0xed], &[0, 2], compiled).await;
        cpu.cpu_state().set_dx(0x80);
        cpu.cpu_state().set_eax(0x1234565a);
        run(&mut cpu).await;
        assert_eq!(cpu.cpu_state().eax(), 0xffffff5a);
        assert_eq!(device.borrow().value, 0x5a);
        assert_eq!((reads.get(), writes.get()), (4, 4));
    }
}

test_backends!(
    low_byte_accesses_invoke_once_and_leave_neighbouring_ports_untouched,
    check_low_byte_accesses
);

async fn check_byte_sequences() {
    for compiled in [false, true] {
        let device = Rc::new(RefCell::new(Registers::default()));
        let weak = Rc::downgrade(&device);
        let mut window_binding = None;
        let io = IoBus::new(device.clone(), |io| {
            let callbacks = DeviceCallbacks::new(&device);
            let window = 0x80..=0x82;
            window_binding = Some(io.bind(
                window.clone(),
                &PortRead::byte_sequence(window.clone(), |_| 0xff),
                &PortWrite::byte_sequence(window.clone(), |_, _| {}),
            ));
            io.bind(
                0x81,
                &PortRead::low_byte(callbacks.read(|_, r| r.value as u8)),
                &PortWrite::low_byte(callbacks.write(|context, r, value: u8| {
                    r.value = value.into();
                    r.tail
                        .as_ref()
                        .unwrap()
                        .set_write(context, r.tail_write.as_ref().unwrap());
                })),
            );
            let tail = io.bind(
                0x82,
                &PortRead::byte_sequence(0x82..=0x83, callbacks.read(|_, r| (r.value >> 8) as u8)),
                &PortWrite::byte_sequence(window.clone(), |_, _| panic!("old tail handler")),
            );
            device.borrow_mut().tail = Some(tail);
            device.borrow_mut().tail_write = Some(PortWrite::byte_sequence(
                0x82..=0x83,
                callbacks.write(|_, r, value| r.value |= u16::from(value) << 8),
            ));
            io.bind(
                0x83,
                &PortRead::byte(|_| panic!("read beyond window")),
                &PortWrite::byte(|_, _| panic!("write beyond window")),
            );
        });
        // A hole starts the transfer. The next operation replaces the final
        // writer, and releases its device borrow before that writer executes.
        // Its low-byte adapter cannot stop the outer sequence, and the final
        // operation's larger window cannot extend the original transfer window.
        io.write_port(0x80, 4, 0x99123400);
        assert_eq!(io.read_port(0x80, 4), 0xff1234ff);
        assert_eq!(io.read_port(0x81, 2), 0xff34);

        let mut cpu = guest(io.clone(), &[0x66, 0xef, 0x66, 0xed], &[0, 2], compiled).await;
        cpu.cpu_state().set_dx(0x80);
        cpu.cpu_state().set_eax(0x99785600);
        run(&mut cpu).await;
        assert_eq!(cpu.cpu_state().eax(), 0xff7856ff);

        // Disabled operations consume the complete access, including its tail.
        let binding = window_binding.unwrap();
        io.with_context(|mut context| {
            binding.set_read(context.reborrow(), &PortRead::new(|_, _, _| u32::MAX));
            binding.set_write(context, &PortWrite::new(|_, _, _, _| {}));
        });
        {
            let registers = device.borrow_mut();
            assert_eq!(io.read_port(0x80, 4), u32::MAX);
            io.write_port(0x80, 4, 0);
            assert_eq!(registers.value, 0x7856);
        }
        drop(cpu);
        drop(io);
        drop(device);
        assert!(weak.upgrade().is_none());
    }
}

test_backends!(
    byte_sequences_resolve_each_operation_after_the_previous_finishes,
    check_byte_sequences
);

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn byte_sequences_and_singletons_never_wrap_at_the_last_port() {
    for window in [0xfffe..=0xffff, 0xffff..=0xffff] {
        let device = Rc::new(RefCell::new(Registers::default()));
        let io = IoBus::new(device.clone(), |io| {
            let callbacks = DeviceCallbacks::new(&device);
            io.bind(
                window.clone(),
                &PortRead::byte_sequence(window.clone(), |_| 0xff),
                &PortWrite::byte_sequence(window.clone(), |_, _| {}),
            );
            io.bind(
                0xffff,
                &PortRead::byte_sequence(window.clone(), callbacks.read(|_, r| r.value as u8)),
                &PortWrite::byte_sequence(
                    window.clone(),
                    callbacks.write(|_, r, value: u8| r.value = value.into()),
                ),
            );
            io.bind(
                0,
                &PortRead::byte(|_| panic!("wrapped read")),
                &PortWrite::byte(|_, _| panic!("wrapped write")),
            );
        });
        io.write_port(0xffff, 4, 0x12345678);
        assert_eq!(device.borrow().value, 0x78);
        assert_eq!(io.read_port(0xffff, 4), 0xffffff78);
    }
}
