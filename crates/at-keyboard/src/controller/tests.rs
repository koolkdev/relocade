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
    let controller = Controller::new(pic.irq(KEYBOARD_IRQ));
    let (commands, _incoming) = async_channel::bounded(1);
    let mut firmware = pin!(controller.run(commands));
    let mut cx = Context::from_waker(Waker::noop());

    // Supply keyboard bytes directly to the controller. Their arrival relative
    // to guest writes must not depend on how often the keyboard task is polled.
    assert!(pin!(controller.send_keyboard(0xf0))
        .poll(&mut cx)
        .is_ready());
    assert_eq!(controller.status() & OUTPUT_BUFFER_FULL, 0);
    controller.write(STATUS_COMMAND_PORT, 0x60); // Write command byte.
    assert!(firmware.as_mut().poll(&mut cx).is_pending());
    controller.write(DATA_PORT, 0x44); // Disable IRQ1, retaining translation.
    assert!(firmware.as_mut().poll(&mut cx).is_pending());
    assert!(pin!(controller.send_keyboard(0x1c))
        .poll(&mut cx)
        .is_ready());
    assert_ne!(controller.status() & OUTPUT_BUFFER_FULL, 0);
    assert_eq!(pic.poll_interrupt(&mut cx), Poll::Pending);
    assert_eq!(controller.read_data(), 0x9e);
}
