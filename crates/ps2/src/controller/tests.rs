use super::*;
use crate::protocol::{mouse, ACK};
use relocade_clock_test_support::ManualClock;
use relocade_frontend::keyboard::{
    ElementState, Key, KeyCode, KeyLocation, KeyboardEvent, NativeKey,
};
use relocade_machine::Device;
use std::{
    future::Future,
    pin::pin,
    rc::Rc,
    task::{Context, Poll, Waker},
};

fn controller(pic: &Pic, with_mouse: bool) -> Controller {
    let clock = ManualClock::default().clock();
    let mouse = with_mouse.then(|| Mouse::new(clock.clone()));
    Controller::new(clock, pic, mouse, None)
}

fn write(controller: &Controller, input: Input) {
    match input {
        Input::Command(command) => controller.write_command(command),
        Input::Data(data) => controller.write_data(data),
    }
    controller.service();
}

fn mouse_command(controller: &Controller, byte: u8) {
    write(controller, Input::Command(WRITE_AUXILIARY));
    write(controller, Input::Data(byte));
}

fn press(controller: &Controller, key: KeyCode) {
    controller.key_event(KeyboardEvent::Key {
        physical_key: key.into(),
        logical_key: Key::Unidentified(NativeKey::Unidentified),
        location: KeyLocation::Standard,
        text: None,
        state: ElementState::Pressed,
        repeat: false,
    });
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn irq_configuration_preserves_a_partially_transmitted_break_sequence() {
    relocade_machine::IoBus::default().with_context(|mut context| {
        let mut pic = Pic::default();
        pic.unmask_irq(KEYBOARD_IRQ);
        let controller = controller(&pic, false);
        let mut cx = Context::from_waker(Waker::noop());

        // Put a guest write precisely between the two wire bytes of a break.
        assert_eq!(controller.firmware.borrow_mut().translate(0xf0), None);
        assert_eq!(controller.status() & OUTPUT_BUFFER_FULL, 0);
        write(&controller, Input::Command(WRITE_COMMAND_BYTE));
        write(&controller, Input::Data(0x44)); // Disable IRQ1, retaining translation.
        press(&controller, KeyCode::KeyA);
        assert_ne!(controller.status() & OUTPUT_BUFFER_FULL, 0);
        assert_eq!(
            pic.poll_interrupt(context.reborrow(), &mut cx),
            Poll::Pending
        );
        assert_eq!(controller.read_data(), 0x9e);
    })
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn auxiliary_bytes_keep_their_source_and_bypass_keyboard_translation() {
    let mut pic = Pic::default();
    let device = Rc::new(RefCell::new(pic.clone()));
    let io = relocade_machine::IoBus::new(device.clone(), |io| Pic::configure_io(io, &device));
    io.with_context(|mut context| {
        for irq in [KEYBOARD_IRQ, relocade_pic::CASCADE_IRQ, MOUSE_IRQ] {
            pic.unmask_irq(irq);
        }
        let controller = controller(&pic, true);
        let mut cx = Context::from_waker(Waker::noop());
        mouse_command(&controller, mouse::SET_WRAP_MODE);
        mouse_command(&controller, 0xf0);
        assert_eq!(
            controller.status() & OUTPUT_BUFFER_FULL,
            0,
            "auxiliary interface starts disabled"
        );
        write(&controller, Input::Command(ENABLE_AUXILIARY));
        assert_eq!(controller.status() & 0x21, 0x21);
        assert_eq!(
            pic.poll_interrupt(context.reborrow(), &mut cx),
            Poll::Pending,
            "IRQ12 starts disabled"
        );
        write(&controller, Input::Command(WRITE_COMMAND_BYTE));
        write(&controller, Input::Data(0x47));
        assert_eq!(
            pic.poll_interrupt(context.reborrow(), &mut cx),
            Poll::Ready(0x74)
        );
        assert_eq!(
            controller.read_data(),
            0xf0,
            "an auxiliary F0 is a data byte"
        );
        assert_eq!(controller.status() & 0x21, 0);
        context.write_port(relocade_pic::SLAVE_COMMAND_PORT, 1, 0x20);
        context.write_port(relocade_pic::COMMAND_PORT, 1, 0x20);
        press(&controller, KeyCode::KeyA);
        assert_eq!(controller.status() & 0x21, 1);
        assert_eq!(
            pic.poll_interrupt(context.reborrow(), &mut cx),
            Poll::Ready(9)
        );
        assert_eq!(controller.read_data(), 0x1e);
        write(&controller, Input::Command(DISABLE_AUXILIARY));
        mouse_command(&controller, 0x12);
        press(&controller, KeyCode::KeyB);
        assert_eq!(
            controller.read_data(),
            0x30,
            "auxiliary inhibit leaves keyboard enabled"
        );
    })
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn full_latch_holds_both_ports_and_replacement_discards_unsent_mouse_bytes() {
    let controller = controller(&Pic::default(), true);
    write(&controller, Input::Command(WRITE_COMMAND_BYTE));
    write(&controller, Input::Data(0x07)); // Enable both ports with raw scan codes.
    write(&controller, Input::Data(0xf2)); // Keyboard identify: FA AB 83.
    mouse_command(&controller, mouse::IDENTIFY); // Mouse identify: FA 00.
    for (source, byte) in [(1, ACK), (0x21, ACK), (1, 0xab), (0x21, 0)] {
        assert_eq!(controller.status() & 0x21, source);
        assert_eq!(controller.read_data(), byte);
        controller.service();
    }

    // The last keyboard byte holds the latch throughout both mouse commands.
    mouse_command(&controller, mouse::SET_WRAP_MODE);
    mouse_command(&controller, 0x34);
    write(&controller, Input::Data(0xee)); // Keyboard echo.
    for (source, byte) in [(1, 0x83), (0x21, 0x34), (1, 0xee)] {
        assert_eq!(controller.status() & 0x21, source);
        assert_eq!(controller.read_data(), byte);
        controller.service();
    }
    assert_eq!(controller.status() & OUTPUT_BUFFER_FULL, 0);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn commands_route_to_separate_ports_and_injected_output_is_source_tagged() {
    let controller = controller(&Pic::default(), true);
    write(&controller, Input::Command(ENABLE_AUXILIARY));
    mouse_command(&controller, mouse::IDENTIFY);
    for byte in [ACK, 0] {
        assert_eq!(controller.status() & 0x21, 0x21);
        assert_eq!(controller.read_data(), byte);
        controller.service();
    }
    write(&controller, Input::Data(0xee));
    assert_eq!(controller.status() & 0x21, 1);
    assert_eq!(controller.read_data(), 0xee);
    write(&controller, Input::Command(WRITE_AUXILIARY));
    write(&controller, Input::Command(WRITE_AUXILIARY_OUTPUT)); // Replace unfinished D4.
    write(&controller, Input::Data(0x1c));
    assert_eq!(controller.status() & 0x21, 0x21);
    assert_eq!(controller.read_data(), 0x1c);
    controller.service();
    assert_eq!(controller.status() & OUTPUT_BUFFER_FULL, 0);
    write(&controller, Input::Command(WRITE_KEYBOARD_OUTPUT));
    write(&controller, Input::Data(0xf0));
    assert_eq!(controller.status() & 0x21, 1);
    assert_eq!(
        controller.read_data(),
        0xf0,
        "injected keyboard bytes bypass translation"
    );
    write(&controller, Input::Command(AUXILIARY_TEST));
    assert_eq!(
        controller.status() & 0x21,
        1,
        "controller replies use the keyboard output source"
    );
    assert_eq!(controller.read_data(), 0);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn an_unattached_auxiliary_port_does_not_block_other_commands() {
    let controller = controller(&Pic::default(), false);
    for byte in [mouse::RESET, mouse::IDENTIFY, mouse::ENABLE_REPORTING] {
        mouse_command(&controller, byte);
        assert_eq!(
            controller.status() & (INPUT_BUFFER_FULL | OUTPUT_BUFFER_FULL),
            0
        );
    }
    write(&controller, Input::Data(0xee));
    assert_eq!(controller.read_data(), 0xee);
    write(&controller, Input::Command(READ_COMMAND_BYTE));
    assert_eq!(controller.read_data(), 0x65);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn readiness_waits_sleep_until_register_changes_and_keep_keyboard_output() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use std::task::Wake;

    #[derive(Default)]
    struct Wakes(AtomicUsize);
    impl Wake for Wakes {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let wakes = Arc::new(Wakes::default());
    let waker = Waker::from(wakes.clone());
    let mut cx = Context::from_waker(&waker);
    let controller = controller(&Pic::default(), false);

    controller.write_command(0xd2);
    let mut input = pin!(controller.wait_input_ready());
    assert!(input.as_mut().poll(&mut cx).is_pending());
    assert_eq!(
        wakes.0.load(Ordering::SeqCst),
        0,
        "a wait must not wake itself"
    );
    controller.service();
    assert!(
        wakes.0.swap(0, Ordering::SeqCst) > 0,
        "IBF clearing wakes the writer"
    );
    assert!(input.as_mut().poll(&mut cx).is_ready());

    let mut auxiliary = pin!(controller.wait_auxiliary_output());
    assert!(auxiliary.as_mut().poll(&mut cx).is_pending());
    assert!(auxiliary.as_mut().poll(&mut cx).is_pending());
    assert_eq!(wakes.0.load(Ordering::SeqCst), 0);
    write(&controller, Input::Data(0x1e));
    assert!(wakes.0.swap(0, Ordering::SeqCst) > 0);
    assert!(auxiliary.as_mut().poll(&mut cx).is_pending());
    assert_eq!(controller.status() & OUTPUT_BUFFER_FULL, OUTPUT_BUFFER_FULL);
    assert_eq!(
        controller.read_data(),
        0x1e,
        "readiness must not consume a keyboard byte"
    );
    assert!(auxiliary.as_mut().poll(&mut cx).is_pending());
    wakes.0.store(0, Ordering::SeqCst);
    write(&controller, Input::Command(0xd3));
    assert!(auxiliary.as_mut().poll(&mut cx).is_pending());
    wakes.0.store(0, Ordering::SeqCst);
    write(&controller, Input::Data(0xfa));
    assert!(
        wakes.0.load(Ordering::SeqCst) > 0,
        "output publication wakes the reader"
    );
    assert!(auxiliary.as_mut().poll(&mut cx).is_ready());
    assert_eq!(controller.read_data(), 0xfa);
}
