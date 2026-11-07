use super::*;
use relocade_machine::Device;
use relocade_pic::{Pic, KEYBOARD_IRQ};
use std::{
    future::Future,
    pin::pin,
    task::{Context, Poll, Waker},
};

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn irq_configuration_preserves_a_partially_transmitted_break_sequence() {
    let mut pic = Pic::default();
    pic.unmask_irq(KEYBOARD_IRQ);
    let controller = Controller::new(&pic, [Port::Keyboard, Port::Auxiliary]);
    let mut firmware = pin!(controller.run());
    let mut cx = Context::from_waker(Waker::noop());

    // Supply keyboard bytes directly to the controller. Their arrival relative
    // to guest writes must not depend on how often the keyboard task is polled.
    assert!(pin!(controller.send(Port::Keyboard, 0xf0))
        .poll(&mut cx)
        .is_ready());
    assert_eq!(controller.status() & OUTPUT_BUFFER_FULL, 0);
    controller.write(STATUS_COMMAND_PORT, 0x60); // Write command byte.
    assert!(firmware.as_mut().poll(&mut cx).is_pending());
    controller.write(DATA_PORT, 0x44); // Disable IRQ1, retaining translation.
    assert!(firmware.as_mut().poll(&mut cx).is_pending());
    assert!(pin!(controller.send(Port::Keyboard, 0x1c))
        .poll(&mut cx)
        .is_ready());
    assert_ne!(controller.status() & OUTPUT_BUFFER_FULL, 0);
    assert_eq!(pic.poll_interrupt(&mut cx), Poll::Pending);
    assert_eq!(controller.read_data(), 0x9e);
}

fn write(
    controller: &Controller,
    firmware: std::pin::Pin<&mut impl Future<Output = ()>>,
    port: u16,
    value: u8,
) {
    controller.write(port, value);
    assert!(firmware
        .poll(&mut Context::from_waker(Waker::noop()))
        .is_pending());
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn auxiliary_bytes_keep_their_source_and_bypass_keyboard_translation() {
    let mut pic = Pic::default();
    for irq in [
        KEYBOARD_IRQ,
        relocade_pic::CASCADE_IRQ,
        relocade_pic::MOUSE_IRQ,
    ] {
        pic.unmask_irq(irq);
    }
    let controller = Controller::new(&pic, [Port::Keyboard, Port::Auxiliary]);
    let mut firmware = pin!(controller.run());
    let mut cx = Context::from_waker(Waker::noop());
    let mut mouse = pin!(controller.send(Port::Auxiliary, 0xf0));
    assert!(
        mouse.as_mut().poll(&mut cx).is_pending(),
        "auxiliary interface starts disabled"
    );
    write(&controller, firmware.as_mut(), STATUS_COMMAND_PORT, 0xa8);
    assert!(mouse.as_mut().poll(&mut cx).is_ready());
    assert_eq!(controller.status() & 0x21, 0x21);
    assert_eq!(
        pic.poll_interrupt(&mut cx),
        Poll::Pending,
        "IRQ12 starts disabled"
    );
    write(&controller, firmware.as_mut(), STATUS_COMMAND_PORT, 0x60);
    write(&controller, firmware.as_mut(), DATA_PORT, 0x47);
    assert_eq!(pic.poll_interrupt(&mut cx), Poll::Ready(0x74));
    assert_eq!(
        controller.read_data(),
        0xf0,
        "auxiliary F0 is not a break prefix"
    );
    assert_eq!(controller.status() & 0x21, 0);
    pic.write_port(relocade_pic::SLAVE_COMMAND_PORT, 1, 0x20);
    pic.write_port(relocade_pic::COMMAND_PORT, 1, 0x20);
    assert!(pin!(controller.send(Port::Keyboard, 0x1c))
        .poll(&mut cx)
        .is_ready());
    assert_eq!(controller.status() & 0x21, 1);
    assert_eq!(pic.poll_interrupt(&mut cx), Poll::Ready(9));
    assert_eq!(controller.read_data(), 0x1e);
    write(&controller, firmware.as_mut(), STATUS_COMMAND_PORT, 0xa7);
    assert!(pin!(controller.send(Port::Auxiliary, 1))
        .poll(&mut cx)
        .is_pending());
    assert!(pin!(controller.send(Port::Keyboard, 0x32))
        .poll(&mut cx)
        .is_ready());
    assert_eq!(
        controller.read_data(),
        0x30,
        "auxiliary inhibit leaves keyboard enabled"
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn both_ports_share_backpressure_and_cancelled_waiters_release_their_turn() {
    let controller = Controller::new(&Pic::default(), [Port::Keyboard, Port::Auxiliary]);
    let mut firmware = pin!(controller.run());
    let mut cx = Context::from_waker(Waker::noop());
    write(&controller, firmware.as_mut(), STATUS_COMMAND_PORT, 0xa8);
    assert!(pin!(controller.send(Port::Keyboard, 0x1c))
        .poll(&mut cx)
        .is_ready());
    let mut keyboard = pin!(controller.send(Port::Keyboard, 0x32));
    let mut mouse = pin!(controller.send(Port::Auxiliary, 0x12));
    assert!(keyboard.as_mut().poll(&mut cx).is_pending());
    assert!(mouse.as_mut().poll(&mut cx).is_pending());
    assert_eq!(controller.read_data(), 0x1e);
    assert!(
        keyboard.as_mut().poll(&mut cx).is_pending(),
        "waiting auxiliary port gets the next turn"
    );
    assert!(mouse.as_mut().poll(&mut cx).is_ready());
    assert_eq!(controller.read_data(), 0x12);
    assert!(keyboard.as_mut().poll(&mut cx).is_ready());
    assert_eq!(controller.read_data(), 0x30);
    // Cancellation removes the pending auxiliary claim, even when it was next.
    assert!(pin!(controller.send(Port::Keyboard, 0x21))
        .poll(&mut cx)
        .is_ready());
    let mut cancelled = Box::pin(controller.send(Port::Auxiliary, 0x34));
    assert!(cancelled.as_mut().poll(&mut cx).is_pending());
    assert_eq!(controller.read_data(), 0x2e);
    drop(cancelled);
    assert!(pin!(controller.send(Port::Keyboard, 0x23))
        .poll(&mut cx)
        .is_ready());
    assert_eq!(controller.read_data(), 0x20);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn commands_route_to_separate_ports_and_injected_output_is_source_tagged() {
    let controller = Controller::new(&Pic::default(), [Port::Keyboard, Port::Auxiliary]);
    let mut firmware = pin!(controller.run());
    let mut cx = Context::from_waker(Waker::noop());
    write(&controller, firmware.as_mut(), STATUS_COMMAND_PORT, 0xd4);
    write(&controller, firmware.as_mut(), DATA_PORT, 0xf2);
    assert_eq!(
        pin!(controller.receive_device(Port::Auxiliary)).poll(&mut cx),
        Poll::Ready(0xf2)
    );
    assert!(pin!(controller.receive_device(Port::Keyboard))
        .poll(&mut cx)
        .is_pending());
    write(&controller, firmware.as_mut(), DATA_PORT, 0xed);
    assert_eq!(
        pin!(controller.receive_device(Port::Keyboard)).poll(&mut cx),
        Poll::Ready(0xed)
    );
    write(&controller, firmware.as_mut(), STATUS_COMMAND_PORT, 0xd4);
    write(&controller, firmware.as_mut(), STATUS_COMMAND_PORT, 0xd3); // Replace unfinished D4.
    write(&controller, firmware.as_mut(), DATA_PORT, 0x1c);
    assert_eq!(controller.status() & 0x21, 0x21);
    assert_eq!(controller.read_data(), 0x1c);
    assert!(pin!(controller.receive_device(Port::Auxiliary))
        .poll(&mut cx)
        .is_pending());
    write(&controller, firmware.as_mut(), STATUS_COMMAND_PORT, 0xd2);
    write(&controller, firmware.as_mut(), DATA_PORT, 0xf0);
    assert_eq!(controller.status() & 0x21, 1);
    assert_eq!(
        controller.read_data(),
        0xf0,
        "injected keyboard bytes are not translated"
    );
    write(&controller, firmware.as_mut(), STATUS_COMMAND_PORT, 0xa9);
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
    let controller = Controller::new(&Pic::default(), [Port::Keyboard]);
    let mut firmware = pin!(controller.run());
    let mut cx = Context::from_waker(Waker::noop());
    for byte in [0xff, 0xf2, 0xf4] {
        write(&controller, firmware.as_mut(), STATUS_COMMAND_PORT, 0xd4);
        write(&controller, firmware.as_mut(), DATA_PORT, byte);
        assert_eq!(controller.status() & INPUT_BUFFER_FULL, 0);
    }
    write(&controller, firmware.as_mut(), DATA_PORT, 0xee);
    assert_eq!(
        pin!(controller.receive_device(Port::Keyboard)).poll(&mut cx),
        Poll::Ready(0xee)
    );
    write(&controller, firmware.as_mut(), STATUS_COMMAND_PORT, 0x20);
    assert_eq!(controller.read_data(), 0x65);
}
