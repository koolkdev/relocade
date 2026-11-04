use super::*;
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

fn event(controller: &mut Controller, event: KeyboardEvent) {
    controller.keyboard.event(event, Duration::ZERO);
    controller.refill();
}

fn drain(controller: &mut Controller) -> Vec<u8> {
    let mut bytes = Vec::new();
    while controller.status() & 1 != 0 {
        bytes.push(controller.read_data());
    }
    bytes
}

fn controller() -> (Controller, relocade_pic::Pic) {
    let pic = relocade_pic::Pic::default();
    pic.unmask_irq(relocade_pic::KEYBOARD_IRQ);
    (Controller::new(pic.irq(relocade_pic::KEYBOARD_IRQ)), pic)
}

fn poll(pic: &mut relocade_pic::Pic) -> Poll<u8> {
    pic.poll_interrupt(&mut Context::from_waker(Waker::noop()))
}

#[test]
fn each_output_byte_has_an_irq_and_reading_consumes_the_register() {
    let (mut controller, mut pic) = controller();
    event(&mut controller, key(KeyCode::ArrowUp, true));
    for expected in [0xe0, 0x48] {
        assert_ne!(controller.status() & 1, 0);
        assert_eq!(poll(&mut pic), Poll::Ready(9));
        assert_eq!(controller.read_data(), expected);
        assert_eq!(poll(&mut pic), Poll::Pending, "next byte waits for EOI");
        pic.write_port(relocade_pic::COMMAND_PORT, 1, 0x20); // Non-specific EOI.
    }
    assert_eq!(controller.status() & 1, 0);
    assert_eq!(poll(&mut pic), Poll::Pending);
    assert_eq!(
        controller.read_data(),
        0xff,
        "empty reads do not replay a scan byte"
    );
    event(&mut controller, key(KeyCode::ArrowUp, false));
    assert_eq!(drain(&mut controller), [0xe0, 0xc8]);
}

#[test]
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

#[test]
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

#[test]
fn keyboard_commands_acknowledge_parameters_and_restore_scanning_after_reset() {
    let (mut controller, _) = controller();
    controller.command(0x20);
    assert_eq!(drain(&mut controller), [0x45]);
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

#[test]
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
    assert_eq!(controller.keyboard.deadline, None);
}

#[test]
fn guest_typematic_programming_owns_repeat_delay_and_rate() {
    let (mut controller, _) = controller();
    controller.data(0xf3);
    assert_eq!(drain(&mut controller), [0xfa]);
    controller.data(0);
    assert_eq!(drain(&mut controller), [0xfa]); // 250ms, 30Hz.
    event(&mut controller, key(KeyCode::KeyA, true));
    assert_eq!(drain(&mut controller), [0x1e]);
    controller.repeat(Duration::from_millis(249));
    controller.refill();
    assert!(drain(&mut controller).is_empty());
    controller.repeat(Duration::from_millis(250));
    controller.refill();
    assert_eq!(drain(&mut controller), [0x1e]);
    assert_eq!(
        controller.keyboard.deadline,
        Some(Duration::from_nanos(283_333_334))
    );
    event(&mut controller, key(KeyCode::KeyA, false));
    assert_eq!(drain(&mut controller), [0x9e]);
    controller.repeat(Duration::from_secs(1));
    controller.refill();
    assert!(drain(&mut controller).is_empty());
}

#[test]
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

#[test]
fn inhibited_typematic_does_not_accumulate_a_burst_and_modifiers_repeat() {
    let (mut controller, _) = controller();
    controller.command(0xad);
    event(&mut controller, key(KeyCode::KeyA, true));
    for second in 1..20 {
        controller.repeat(Duration::from_secs(second));
    }
    controller.command(0xae);
    assert_eq!(drain(&mut controller), [0x1e]);
    event(&mut controller, key(KeyCode::ShiftLeft, true));
    controller.repeat(Duration::from_secs(20));
    controller.repeat(Duration::from_secs(21));
    assert_eq!(
        drain(&mut controller),
        [0x2a],
        "full output inhibits repeat"
    );
    controller.repeat(Duration::from_secs(22));
    assert_eq!(
        drain(&mut controller),
        [0x2a],
        "the most recent key repeats"
    );
    event(&mut controller, key(KeyCode::Pause, true));
    assert_eq!(drain(&mut controller), [0xe1, 0x1d, 0x45, 0xe1, 0x9d, 0xc5]);
    controller.repeat(Duration::from_secs(23));
    assert_eq!(
        drain(&mut controller),
        [0x2a],
        "Pause does not take repeat ownership"
    );
}

#[test]
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
