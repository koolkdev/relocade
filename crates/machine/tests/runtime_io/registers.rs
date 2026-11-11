//! Standalone registration, window transfer policies and endpoint ownership.

use super::*;
use relocade_machine::{BytePort, DeviceCallbacks, PortPolicy, PortWrite};

#[derive(Default)]
struct Registers {
    value: u16,
    index: u16,
    tail: Option<BytePort>,
    tail_write: Option<PortWrite>,
}

impl Device for Registers {}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn exact_register_widths_reject_access_before_device_borrow() {
    let device = Rc::new(RefCell::new(Registers::default()));
    let mut saved_word = None;
    let io = IoBus::new(device.clone(), |io| {
        let callbacks = DeviceCallbacks::new(&device);
        io.port(
            0x80,
            PortPolicy::BYTE,
            callbacks.read(|_, registers| registers.value as u8),
            callbacks.write(|_, registers, value: u8| registers.value = value.into()),
        );
        saved_word = Some(io.port(
            0x102,
            PortPolicy::WORD,
            callbacks.read(|_, registers| registers.value),
            callbacks.write(|_, registers, value| registers.value = value),
        ));
        let mut words = io.ports(0x100..=0x101, PortPolicy::WORD);
        let mut pair = words.indexed(0x100, 0x101).index(
            callbacks.read(|_, registers| registers.index),
            callbacks.write(|_, registers, value| registers.index = value),
        );
        pair.data = pair.data.define(
            callbacks.read(|_, registers| registers.value),
            callbacks.write(|_, registers, value| registers.value = value),
        );
        words.configure(io.context(), true, |context| {
            pair.configure(context, &pair.data.read, &pair.data.write);
        });
    });
    io.write_port(0x100, 2, 7);
    io.write_port(0x101, 2, 0x1234);
    assert_eq!(io.read_port(0x100, 2), 7);
    assert_eq!(io.read_port(0x101, 2), 0x1234);
    assert_eq!(io.read_port(0x102, 2), 0x1234);
    assert_eq!(io.read_port(0x80, 1), 0x34);
    {
        let registers = device.borrow_mut();
        for (port, bytes, open_bus) in [
            (0x80, 2, 0xffff),
            (0x80, 4, u32::MAX),
            (0x100, 1, 0xff),
            (0x100, 4, u32::MAX),
            (0x102, 1, 0xff),
            (0x102, 4, u32::MAX),
        ] {
            assert_eq!(io.read_port(port, bytes), open_bus);
            io.write_port(port, bytes, 0);
        }
        assert_eq!(registers.value, 0x1234);
        assert_eq!(registers.index, 7);
    }
    io.write_port(0x80, 1, 0x56);
    assert_eq!(io.read_port(0x101, 2), 0x56);
    io.write_port(0x102, 2, 0x7856);
    assert_eq!(io.read_port(0x102, 2), 0x7856);

    // The returned endpoint can select a cached operation after setup ends.
    let word = saved_word.unwrap();
    let read = word.read_handler(|_| 0xabcd);
    io.with_context(|context| word.set_read(context, &read));
    assert_eq!(io.read_port(0x102, 2), 0xabcd);
    io.write_port(0x102, 2, 0x1234);
    assert_eq!(io.read_port(0x101, 2), 0x1234);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn byte_sequences_visit_holes_and_observe_preceding_handler_changes() {
    let device = Rc::new(RefCell::new(Registers::default()));
    let weak = Rc::downgrade(&device);
    let mut saved_window = None;
    let mut saved_pair = None;
    let io = IoBus::new(device.clone(), |io| {
        let callbacks = DeviceCallbacks::new(&device);
        let mut window = io.ports(0x80..=0x82, PortPolicy::BYTE_SEQUENCE);
        let mut pair = window.indexed(0x81, 0x82).index(
            callbacks.read(|_, registers| registers.value as u8),
            callbacks.write(|context, registers, value: u8| {
                registers.value = value.into();
                registers
                    .tail
                    .as_ref()
                    .unwrap()
                    .set_write(context, registers.tail_write.as_ref().unwrap());
            }),
        );
        pair.data = pair.data.define(
            callbacks.read(|_, registers| (registers.value >> 8) as u8),
            |_, _| panic!("old tail handler"),
        );
        let tail_write = pair
            .data
            .write_handler(callbacks.write(|_, registers, value| {
                registers.value |= u16::from(value) << 8;
            }));
        device.borrow_mut().tail = Some(pair.data.clone());
        device.borrow_mut().tail_write = Some(tail_write);
        window.configure(io.context(), true, |context| {
            pair.configure(context, &pair.data.read, &pair.data.write);
        });
        saved_window = Some(window);
        saved_pair = Some(pair);
    });
    io.connect(Rc::new(RefCell::new(Registers::default())), |io| {
        io.port(
            0x83,
            PortPolicy::BYTE_SEQUENCE,
            |_| panic!("read beyond window"),
            |_, _| panic!("write beyond window"),
        );
    });
    // Starts at an unspecified register. Its tail reaches two concrete operations;
    // the first replaces the second while holding the same device's mutable borrow.
    io.write_port(0x80, 4, 0x99123400);
    assert_eq!(device.borrow().value, 0x1234);
    assert_eq!(io.read_port(0x80, 4), 0xff1234ff);
    assert_eq!(io.read_port(0x81, 2), 0x1234);

    let window = saved_window.unwrap();
    let pair = saved_pair.unwrap();
    // Disabling the whole window also disables holes and complete wide transfers.
    io.with_context(|context| {
        window.configure(context, false, |_| panic!("disabled configuration"))
    });
    {
        let registers = device.borrow_mut();
        assert_eq!(io.read_port(0x80, 4), u32::MAX);
        io.write_port(0x80, 4, 0);
        assert_eq!(registers.value, 0x1234);
    }
    io.with_context(|context| {
        window.configure(context, true, |context| {
            pair.configure(context, &pair.data.read, &pair.data.write);
        });
    });
    io.write_port(0x80, 4, 0x99785600);
    assert_eq!(io.read_port(0x80, 4), 0xff7856ff);
    drop(io);
    drop(device);
    assert!(
        weak.upgrade().is_none(),
        "retained endpoints and callbacks must not keep the device alive"
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn byte_window_at_last_port_never_wraps_into_another_device() {
    let device = Rc::new(RefCell::new(Registers::default()));
    let io = IoBus::new(device.clone(), |io| {
        let callbacks = DeviceCallbacks::new(&device);
        let mut window = io.ports(0xfffe..=0xffff, PortPolicy::BYTE_SEQUENCE);
        let last = window.port(0xffff).define(
            callbacks.read(|_, registers| registers.value as u8),
            callbacks.write(|_, registers, value: u8| registers.value = value.into()),
        );
        window.configure(io.context(), true, |context| {
            last.configure(context, &last.read, &last.write);
        });
        io.port(
            0,
            PortPolicy::BYTE,
            |_| panic!("wrapped read"),
            |_, _| panic!("wrapped write"),
        );
    });
    io.write_port(0xfffe, 4, 0x12345678);
    assert_eq!(device.borrow().value, 0x56);
    assert_eq!(io.read_port(0xfffe, 4), 0xffff56ff);
}
