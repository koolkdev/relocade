#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

mod fixture;
mod mouse;
use super::*;
use fixture::Fixture;
use relocade_frontend::keyboard::{
    ElementState, Key, KeyCode, KeyLocation, KeyboardEvent, ModifiersState, NativeKey,
};
use std::{
    task::{Context, Poll, Waker},
    time::Duration,
};

fn key(code: KeyCode, pressed: bool) -> KeyboardEvent {
    KeyboardEvent::Key {
        physical_key: code.into(),
        logical_key: Key::Unidentified(NativeKey::Unidentified),
        location: KeyLocation::Standard,
        text: Some("ש".into()),
        state: if pressed {
            ElementState::Pressed
        } else {
            ElementState::Released
        },
        repeat: false,
    }
}

fn event(controller: &mut Fixture, event: KeyboardEvent) {
    controller.event(event);
}

fn drain(controller: &mut Fixture) -> Vec<u8> {
    let mut bytes = Vec::new();
    while controller.status() & 1 != 0 {
        bytes.push(controller.read_data());
    }
    bytes
}

fn controller() -> (Fixture, relocade_pic::Pic) {
    let pic = relocade_pic::Pic::default();
    pic.unmask_irq(relocade_pic::KEYBOARD_IRQ);
    (Fixture::new(&pic), pic)
}

fn poll(pic: &mut relocade_pic::Pic) -> Poll<u8> {
    relocade_machine::IoBus::default().with_context(|mut context| {
        pic.poll_interrupt(context.reborrow(), &mut Context::from_waker(Waker::noop()))
    })
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn each_output_byte_has_an_irq_and_reads_preserve_the_data_latch() {
    relocade_machine::IoBus::default().with_context(|mut context| {
        let (mut controller, mut pic) = controller();
        event(&mut controller, key(KeyCode::ArrowUp, true));
        for expected in [0xe0, 0x48] {
            assert_ne!(controller.status() & 1, 0);
            assert_eq!(poll(&mut pic), Poll::Ready(9));
            assert_eq!(controller.read_unpolled(), expected);
            assert_eq!(controller.status() & OUTPUT_BUFFER_FULL, 0);
            assert_eq!(
                controller.read_unpolled(),
                expected,
                "a chained reader sees the latch"
            );
            assert_eq!(controller.status() & OUTPUT_BUFFER_FULL, 0);
            controller.settle();
            assert_eq!(poll(&mut pic), Poll::Pending, "next byte waits for EOI");
            pic.write_port(context.reborrow(), relocade_pic::COMMAND_PORT, 1, 0x20);
            // Non-specific EOI.
        }
        assert_eq!(controller.status() & 1, 0);
        assert_eq!(poll(&mut pic), Poll::Pending);
        assert_eq!(
            controller.read_data(),
            0x48,
            "an empty output still exposes its last value"
        );
        assert_eq!(controller.status() & OUTPUT_BUFFER_FULL, 0);
        assert_eq!(
            poll(&mut pic),
            Poll::Pending,
            "rereading does not raise an IRQ"
        );
        event(&mut controller, key(KeyCode::ArrowUp, false));
        assert_eq!(drain(&mut controller), [0xe0, 0xc8]);
    })
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn disabling_irq_retains_pollable_data_and_interface_disable_buffers_keys() {
    let (mut controller, mut pic) = controller();
    controller.command(0x60);
    controller.data(0x44);
    event(&mut controller, key(KeyCode::KeyA, true));
    assert_eq!(poll(&mut pic), Poll::Pending);
    assert_eq!(controller.read_data(), 0x1e);
    controller.command(0xad);
    event(&mut controller, key(KeyCode::KeyA, false));
    assert_eq!(controller.status() & 1, 0);
    controller.command(0xae);
    assert_ne!(controller.status() & 1, 0);
    controller.command(0x60);
    controller.data(0x45);
    assert_eq!(
        poll(&mut pic),
        Poll::Ready(9),
        "enabling IRQ with full OBF asserts IRQ1"
    );
    assert_eq!(controller.read_data(), 0x9e);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn raw_scan_sets_controller_translation_and_special_sequences_are_distinct() {
    let (mut controller, _) = controller();
    for (code, expected) in [
        (
            KeyCode::PrintScreen,
            &[0xe0, 0x2a, 0xe0, 0x37, 0xe0, 0xb7, 0xe0, 0xaa][..],
        ),
        (KeyCode::Pause, &[0xe1, 0x1d, 0x45, 0xe1, 0x9d, 0xc5][..]),
        (KeyCode::ControlRight, &[0xe0, 0x1d, 0xe0, 0x9d][..]),
        (KeyCode::F11, &[0x57, 0xd7][..]),
    ] {
        event(&mut controller, key(code, true));
        event(&mut controller, key(code, false));
        assert_eq!(drain(&mut controller), expected);
    }
    controller.command(0x60);
    controller.data(5);
    event(&mut controller, key(KeyCode::KeyA, true));
    event(&mut controller, key(KeyCode::KeyA, false));
    assert_eq!(drain(&mut controller), [0x1c, 0xf0, 0x1c]);
    controller.data(0xf0);
    assert_eq!(drain(&mut controller), [0xfa]);
    controller.data(1);
    assert_eq!(drain(&mut controller), [0xfa]);
    event(&mut controller, key(KeyCode::KeyA, true));
    event(&mut controller, key(KeyCode::KeyA, false));
    assert_eq!(drain(&mut controller), [0x1e, 0x9e]);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn keyboard_commands_acknowledge_parameters_and_restore_scanning_after_reset() {
    let (mut controller, _) = controller();
    controller.command(0x20);
    assert_eq!(drain(&mut controller), [0x65]);
    controller.command(0xaa);
    assert_eq!(drain(&mut controller), [0x55]);
    controller.command(0xab);
    assert_eq!(drain(&mut controller), [0]);
    for (command, expected) in [
        (0xf2, &[0xfa, 0xab, 0x41][..]),
        (0xee, &[0xee]),
        (0xf1, &[0xfe]),
    ] {
        controller.data(command);
        assert_eq!(drain(&mut controller), expected);
    }
    controller.data(0xed);
    assert_eq!(drain(&mut controller), [0xfa]);
    controller.data(7);
    assert_eq!(drain(&mut controller), [0xfa]);
    controller.data(0xfe);
    assert_eq!(drain(&mut controller), [0xfa]);
    controller.data(0xf0);
    assert_eq!(drain(&mut controller), [0xfa]);
    controller.data(0);
    assert_eq!(drain(&mut controller), [0xfa, 0x41]);
    controller.data(0xff);
    assert_eq!(drain(&mut controller), [0xfa, 0xaa]);
    event(&mut controller, key(KeyCode::KeyB, true));
    event(&mut controller, key(KeyCode::KeyB, false));
    assert_eq!(drain(&mut controller), [0x30, 0xb0]);
    controller.data(0xf5); // Default-disable is distinct from successful reset.
    assert_eq!(drain(&mut controller), [0xfa]);
    event(&mut controller, key(KeyCode::KeyA, true));
    event(&mut controller, key(KeyCode::KeyA, false));
    assert!(drain(&mut controller).is_empty());
    controller.data(0xf4);
    assert_eq!(drain(&mut controller), [0xfa]);
    event(&mut controller, key(KeyCode::KeyA, true));
    assert_eq!(drain(&mut controller), [0x1e]);
    controller.command(0xd2);
    controller.data(0x9e);
    assert_eq!(drain(&mut controller), [0x9e]);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn focus_and_modifier_snapshots_emit_physical_transitions_without_duplicates() {
    let (mut controller, _) = controller();
    event(
        &mut controller,
        KeyboardEvent::Modifiers(ModifiersState::CONTROL),
    );
    assert_eq!(drain(&mut controller), [0x1d]);
    event(
        &mut controller,
        KeyboardEvent::Modifiers(ModifiersState::CONTROL),
    );
    event(&mut controller, key(KeyCode::KeyA, true));
    event(&mut controller, key(KeyCode::KeyA, true));
    assert_eq!(
        drain(&mut controller),
        [0x1e],
        "host repeat does not duplicate make codes"
    );
    event(&mut controller, KeyboardEvent::FocusLost);
    let mut releases = drain(&mut controller);
    releases.sort();
    assert_eq!(releases, [0x9d, 0x9e]);
    controller.advance(Duration::from_secs(1));
    assert!(drain(&mut controller).is_empty());
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn guest_typematic_programming_owns_repeat_delay_and_rate() {
    let (mut controller, _) = controller();
    controller.data(0xf3);
    assert_eq!(drain(&mut controller), [0xfa]);
    controller.data(0);
    assert_eq!(drain(&mut controller), [0xfa]); // 250ms, 30Hz.
    event(&mut controller, key(KeyCode::KeyA, true));
    assert_eq!(drain(&mut controller), [0x1e]);
    controller.advance(Duration::from_millis(249));
    assert!(drain(&mut controller).is_empty());
    controller.advance(Duration::from_millis(250));
    assert_eq!(drain(&mut controller), [0x1e]);
    controller.advance(Duration::from_nanos(283_333_333));
    assert!(drain(&mut controller).is_empty());
    controller.advance(Duration::from_nanos(283_333_334));
    assert_eq!(drain(&mut controller), [0x1e]);
    event(&mut controller, key(KeyCode::KeyA, false));
    assert_eq!(drain(&mut controller), [0x9e]);
    controller.advance(Duration::from_secs(1));
    assert!(drain(&mut controller).is_empty());
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn commands_abort_pending_options_and_resend_preserves_them() {
    let (mut controller, _) = controller();
    controller.data(0xed);
    assert_eq!(drain(&mut controller), [0xfa]);
    controller.data(0xfe);
    assert_eq!(drain(&mut controller), [0xfa]);
    controller.data(3);
    assert_eq!(drain(&mut controller), [0xfa]);
    controller.data(0xed);
    assert_eq!(drain(&mut controller), [0xfa]);
    controller.data(0xff);
    assert_eq!(drain(&mut controller), [0xfa, 0xaa]);
    controller.data(0xf3);
    assert_eq!(drain(&mut controller), [0xfa]);
    controller.data(0xf4);
    assert_eq!(drain(&mut controller), [0xfa]);
    event(&mut controller, key(KeyCode::KeyA, true));
    assert_eq!(drain(&mut controller), [0x1e]);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn inhibited_typematic_does_not_accumulate_a_burst_and_modifiers_repeat() {
    let (mut controller, _) = controller();
    controller.command(0xad);
    event(&mut controller, key(KeyCode::KeyA, true));
    for second in 1..20 {
        controller.advance(Duration::from_secs(second));
    }
    controller.command(0xae);
    assert_eq!(drain(&mut controller), [0x1e]);
    event(&mut controller, key(KeyCode::ShiftLeft, true));
    controller.advance(Duration::from_secs(20));
    controller.advance(Duration::from_secs(21));
    assert_eq!(
        drain(&mut controller),
        [0x2a],
        "full output inhibits repeat"
    );
    controller.advance(Duration::from_secs(22));
    assert_eq!(
        drain(&mut controller),
        [0x2a],
        "the most recent key repeats"
    );
    event(&mut controller, key(KeyCode::Pause, true));
    assert_eq!(drain(&mut controller), [0xe1, 0x1d, 0x45, 0xe1, 0x9d, 0xc5]);
    controller.advance(Duration::from_secs(23));
    assert_eq!(
        drain(&mut controller),
        [0x2a],
        "Pause does not take repeat ownership"
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn overflowing_the_keyboard_fifo_reports_overrun_without_partial_sequences() {
    let (mut controller, _) = controller();
    controller.command(0xad);
    for _ in 0..5 {
        event(&mut controller, key(KeyCode::KeyA, true));
        event(&mut controller, key(KeyCode::KeyA, false));
    }
    // Fifteen raw set-2 bytes leave no room for an extended make sequence.
    event(&mut controller, key(KeyCode::ArrowUp, true));
    controller.command(0xae);
    assert_eq!(
        drain(&mut controller),
        [0x1e, 0x9e, 0x1e, 0x9e, 0x1e, 0x9e, 0x1e, 0x9e, 0x1e, 0x9e, 0xff]
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn input_register_stays_busy_until_firmware_consumes_the_accepted_byte() {
    let (mut controller, _) = controller();
    controller.write_unpolled(STATUS_COMMAND_PORT, 0x20); // Read command byte.
    assert_ne!(controller.status() & INPUT_BUFFER_FULL, 0);
    assert_eq!(controller.status() & OUTPUT_BUFFER_FULL, 0);
    controller.write_unpolled(DATA_PORT, 0xff); // Busy writes cannot replace it.
    assert_ne!(controller.status() & INPUT_BUFFER_FULL, 0);
    controller.settle();
    assert_eq!(controller.status() & INPUT_BUFFER_FULL, 0);
    assert_eq!(controller.read_unpolled(), 0x65);
    assert_eq!(controller.status() & OUTPUT_BUFFER_FULL, 0);
    controller.settle();
    assert!(drain(&mut controller).is_empty());

    controller.command(0x60); // A new controller command replaces its option wait.
    controller.command(0x20);
    assert_eq!(drain(&mut controller), [0x65]);
    controller.data(0xee);
    assert_eq!(drain(&mut controller), [0xee]);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn replies_wait_for_output_space_and_new_commands_interrupt_untransmitted_acks() {
    let (mut controller, _) = controller();
    event(&mut controller, key(KeyCode::KeyA, true));
    controller.data(0xed); // LED ACK waits behind the unread make byte.
    controller.data(0xee); // ECHO cancels that ACK and option exchange.
    controller.command(0xad); // Controller commands still run while output waits.
    assert_eq!(controller.status() & INPUT_BUFFER_FULL, 0);
    assert_eq!(controller.read_unpolled(), 0x1e);
    assert_eq!(controller.status() & OUTPUT_BUFFER_FULL, 0);
    controller.settle();
    assert_eq!(controller.status() & OUTPUT_BUFFER_FULL, 0);
    controller.command(0xae);
    assert_eq!(drain(&mut controller), [0xee]);

    controller.data(0xf2); // Three-byte ID reply advances only as the guest reads.
    event(&mut controller, key(KeyCode::KeyB, true));
    event(&mut controller, key(KeyCode::KeyB, false));
    assert_eq!(drain(&mut controller), [0xfa, 0xab, 0x41]);
    event(&mut controller, key(KeyCode::KeyC, true));
    assert_eq!(drain(&mut controller), [0x2e], "scanning resumes after ID");
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn unread_output_is_preserved_when_queued_input_overflows_the_keyboard() {
    let (mut controller, _) = controller();
    event(&mut controller, key(KeyCode::KeyB, true));
    for _ in 0..32 {
        controller.queue_event(key(KeyCode::KeyA, true));
        controller.queue_event(key(KeyCode::KeyA, false));
    }
    controller.settle();
    // The unread B make stays in the controller. Sixteen keyboard bytes fit
    // five set-2 make/break pairs and one make; excess input reports overrun.
    let mut expected = vec![0x30];
    expected.extend([0x1e, 0x9e].repeat(5));
    expected.extend([0x1e, 0xff]);
    assert_eq!(drain(&mut controller), expected);
    event(&mut controller, key(KeyCode::KeyA, true));
    event(&mut controller, key(KeyCode::KeyA, false));
    assert_eq!(drain(&mut controller), [0x1e, 0x9e]);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn disconnect_releases_held_keys_and_keeps_guest_command_service_alive() {
    let (mut controller, _) = controller();
    event(&mut controller, key(KeyCode::KeyA, true));
    assert_eq!(drain(&mut controller), [0x1e]);
    controller.disconnect();
    assert_eq!(drain(&mut controller), [0x9e]);
    controller.advance(Duration::from_secs(1));
    assert!(drain(&mut controller).is_empty());
    controller.data(0xf2);
    assert_eq!(drain(&mut controller), [0xfa, 0xab, 0x41]);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn finishing_an_option_exchange_rescans_keys_released_while_scanning_was_paused() {
    let (mut controller, _) = controller();
    event(&mut controller, key(KeyCode::ShiftLeft, true));
    assert_eq!(drain(&mut controller), [0x2a]);
    controller.data(0xed);
    assert_eq!(drain(&mut controller), [0xfa]);
    event(&mut controller, KeyboardEvent::FocusLost);
    assert!(drain(&mut controller).is_empty());
    controller.data(0xfe); // Retrying the ACK preserves the suspended exchange.
    assert_eq!(drain(&mut controller), [0xfa]);
    controller.data(0);
    assert_eq!(drain(&mut controller), [0xfa, 0xaa]);
    controller.advance(Duration::from_secs(1));
    assert!(drain(&mut controller).is_empty());
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn a_reset_replaced_before_its_ack_preserves_the_programmed_scan_set() {
    let (mut controller, _) = controller();
    controller.command(0x60);
    controller.data(5); // Disable translation to observe the selected scan set.
    controller.data(0xf0);
    assert_eq!(drain(&mut controller), [0xfa]);
    controller.data(1);
    assert_eq!(drain(&mut controller), [0xfa]);
    event(&mut controller, key(KeyCode::KeyA, true));
    controller.data(0xff); // Reset ACK waits behind the unread make byte.
    controller.data(0xee); // Replace reset before its ACK is transmitted.
    assert_eq!(drain(&mut controller), [0x1e, 0xee]);
    controller.data(0xf0);
    assert_eq!(drain(&mut controller), [0xfa]);
    controller.data(0);
    assert_eq!(drain(&mut controller), [0xfa, 1]);
}
