use super::*;
use relocade_frontend::mouse::{MouseButton, MouseEvent};

fn mouse() -> Fixture {
    let mut fixture = Fixture::new(&Pic::default());
    fixture.command(0xa8);
    fixture
}

fn command(fixture: &mut Fixture, byte: u8) -> Vec<u8> {
    fixture.mouse_command(byte);
    let mut reply = Vec::new();
    while fixture.status() & OUTPUT_BUFFER_FULL != 0 {
        assert_ne!(fixture.status() & AUXILIARY_OUTPUT, 0);
        reply.push(fixture.read_data());
    }
    reply
}

fn motion(fixture: &mut Fixture, x: f64, y: f64) {
    fixture.mouse_event(MouseEvent::Motion {
        delta_x: x,
        delta_y: y,
    });
}

fn button(fixture: &mut Fixture, button: MouseButton, state: ElementState) {
    fixture.mouse_event(MouseEvent::Button { button, state });
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn reset_identification_settings_and_resend_are_auxiliary_replies() {
    let mut mouse = mouse();
    assert_eq!(command(&mut mouse, 0xff), [0xfa, 0xaa, 0]);
    assert_eq!(command(&mut mouse, 0xf2), [0xfa, 0]);
    assert_eq!(command(&mut mouse, 0xe9), [0xfa, 0, 2, 100]);
    assert_eq!(command(&mut mouse, 0xf3), [0xfa]);
    assert_eq!(command(&mut mouse, 200), [0xfa]);
    assert_eq!(command(&mut mouse, 0xe8), [0xfa]);
    assert_eq!(command(&mut mouse, 3), [0xfa]);
    assert_eq!(command(&mut mouse, 0xe7), [0xfa]);
    assert_eq!(command(&mut mouse, 0xf0), [0xfa]);
    button(&mut mouse, MouseButton::Left, ElementState::Pressed);
    button(&mut mouse, MouseButton::Middle, ElementState::Pressed);
    assert_eq!(command(&mut mouse, 0xe9), [0xfa, 0x56, 3, 200]);
    assert_eq!(
        command(&mut mouse, 0xfe),
        [0x56, 3, 200],
        "ACK is not replayed"
    );
    assert_eq!(command(&mut mouse, 0xf6), [0xfa]);
    assert_eq!(command(&mut mouse, 0xe9), [0xfa, 6, 2, 100]);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn remote_packets_preserve_fractional_motion_signs_and_saturate_counters() {
    let mut mouse = mouse();
    assert_eq!(command(&mut mouse, 0xf0), [0xfa]);
    motion(&mut mouse, 0.25, 0.25);
    assert_eq!(command(&mut mouse, 0xeb), [0xfa, 8, 0, 0]);
    motion(&mut mouse, 0.875, 0.875);
    assert_eq!(command(&mut mouse, 0xeb), [0xfa, 0x28, 1, 0xff]);
    motion(&mut mouse, 0.875, 0.875);
    assert_eq!(command(&mut mouse, 0xeb), [0xfa, 0x28, 1, 0xff]);
    motion(&mut mouse, -256.0, -255.0);
    assert_eq!(command(&mut mouse, 0xeb), [0xfa, 0x18, 0, 0xff]);
    motion(&mut mouse, 1000.0, 1000.0);
    assert_eq!(command(&mut mouse, 0xeb), [0xfa, 0xe8, 0xff, 0]);
    assert_eq!(
        command(&mut mouse, 0xeb),
        [0xfa, 8, 0, 0],
        "Read Data clears counters"
    );
    assert_eq!(command(&mut mouse, 0xe8), [0xfa]);
    assert_eq!(command(&mut mouse, 0), [0xfa]);
    motion(&mut mouse, 3.0, 0.0);
    motion(&mut mouse, 1.0, 0.0);
    assert_eq!(command(&mut mouse, 0xeb), [0xfa, 8, 1, 0]);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sampling_coalesces_motion_but_keeps_clicks_and_focus_loss_ordered() {
    let mut mouse = mouse();
    assert_eq!(command(&mut mouse, 0xf4), [0xfa]);
    motion(&mut mouse, 0.5, 0.0);
    mouse.advance(Duration::from_millis(10));
    assert!(drain(&mut mouse).is_empty());
    motion(&mut mouse, 1.5, 0.0);
    assert_eq!(drain(&mut mouse), [8, 2, 0]);
    button(&mut mouse, MouseButton::Left, ElementState::Pressed);
    button(&mut mouse, MouseButton::Left, ElementState::Released);
    mouse.advance(Duration::from_millis(19));
    assert!(drain(&mut mouse).is_empty());
    mouse.advance(Duration::from_millis(20));
    assert_eq!(drain(&mut mouse), [9, 0, 0]);
    mouse.advance(Duration::from_millis(30));
    assert_eq!(drain(&mut mouse), [8, 0, 0]);
    button(&mut mouse, MouseButton::Right, ElementState::Pressed);
    button(&mut mouse, MouseButton::Middle, ElementState::Pressed);
    mouse.mouse_event(MouseEvent::FocusLost);
    for (millis, header) in [(40, 10), (50, 14), (60, 8)] {
        mouse.advance(Duration::from_millis(millis));
        assert_eq!(drain(&mut mouse), [header, 0, 0]);
    }
    mouse.advance(Duration::from_secs(1));
    assert!(
        drain(&mut mouse).is_empty(),
        "idle mouse does not emit timer packets"
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn command_replacement_wrap_and_invalid_parameters_keep_protocol_boundaries() {
    let mut mouse = mouse();
    assert_eq!(command(&mut mouse, 0xf3), [0xfa]);
    assert_eq!(command(&mut mouse, 0), [0xfe]);
    assert_eq!(command(&mut mouse, 1), [0xfc]);
    assert_eq!(command(&mut mouse, 20), [0xfa]);
    assert_eq!(command(&mut mouse, 0xf3), [0xfa]);
    assert_eq!(command(&mut mouse, 0xf1), [0xfe]);
    assert_eq!(
        command(&mut mouse, 20),
        [0xfa],
        "invalid command preserves the parameter wait"
    );
    assert_eq!(command(&mut mouse, 0xe8), [0xfa]);
    assert_eq!(
        command(&mut mouse, 0xf2),
        [0xfa, 0],
        "ID replaces unfinished resolution command"
    );
    assert_eq!(command(&mut mouse, 0xe9), [0xfa, 0, 2, 20]);
    assert_eq!(command(&mut mouse, 0), [0xfe]);
    assert_eq!(command(&mut mouse, 0), [0xfc]);
    assert_eq!(command(&mut mouse, 0xee), [0xfa]);
    assert_eq!(command(&mut mouse, 0xf2), [0xf2], "wrap echoes commands");
    assert_eq!(command(&mut mouse, 0xec), [0xfa]);
    assert_eq!(command(&mut mouse, 0xf2), [0xfa, 0]);

    mouse.mouse_command(0xff); // Leave reset ACK in the output latch.
    mouse.mouse_command(0xe9); // Ignored until BAT/ID completes.
    assert_eq!(drain(&mut mouse), [0xfa, 0xaa, 0]);
    assert_eq!(command(&mut mouse, 0xe9), [0xfa, 0, 2, 100]);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn backpressure_preserves_clicks_and_commands_cancel_only_untransmitted_packet_bytes() {
    let mut mouse = mouse();
    assert_eq!(command(&mut mouse, 0xf4), [0xfa]);
    motion(&mut mouse, 2.0, -3.0);
    mouse.advance(Duration::from_millis(10)); // Header is unread, X/Y are waiting.
    button(&mut mouse, MouseButton::Left, ElementState::Pressed);
    button(&mut mouse, MouseButton::Left, ElementState::Released);
    motion(&mut mouse, 1.0, 0.0);
    motion(&mut mouse, 3.0, 0.0);
    mouse.advance(Duration::from_millis(100));
    // The late sample emits one next report, then starts a fresh sample period.
    assert_eq!(drain(&mut mouse), [8, 2, 3, 9, 0, 0]);
    mouse.advance(Duration::from_millis(110));
    assert_eq!(drain(&mut mouse), [8, 4, 0]);
    assert_eq!(
        command(&mut mouse, 0xfe),
        [8, 4, 0],
        "resend repeats a complete packet"
    );
    motion(&mut mouse, 5.0, 6.0);
    mouse.advance(Duration::from_millis(120));
    mouse.mouse_command(0xf2);
    assert_eq!(
        drain(&mut mouse),
        [0x28, 0xfa, 0],
        "published header remains; command replaces pending X/Y"
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn stream_scaling_sample_rate_and_remote_read_data_are_distinct() {
    let mut mouse = mouse();
    for byte in [0xf3, 200, 0xe7, 0xf4] {
        assert_eq!(command(&mut mouse, byte), [0xfa]);
    }
    motion(&mut mouse, 4.0, 2.0);
    mouse.advance(Duration::from_millis(4));
    assert!(drain(&mut mouse).is_empty());
    mouse.advance(Duration::from_millis(5));
    assert_eq!(drain(&mut mouse), [0x28, 6, 0xff]);
    motion(&mut mouse, 4.0, 2.0);
    assert_eq!(
        command(&mut mouse, 0xeb),
        [0xfa, 0x28, 4, 0xfe],
        "Read Data bypasses 2:1 scaling"
    );
    assert_eq!(command(&mut mouse, 0xf5), [0xfa]);
    motion(&mut mouse, 20.0, 20.0);
    mouse.advance(Duration::from_secs(1));
    assert!(drain(&mut mouse).is_empty(), "disabled stream stays quiet");
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn disconnect_and_configuration_waits_release_previously_reported_buttons() {
    let mut mouse = mouse();
    assert_eq!(command(&mut mouse, 0xf4), [0xfa]);
    button(&mut mouse, MouseButton::Left, ElementState::Pressed);
    mouse.advance(Duration::from_millis(10));
    assert_eq!(drain(&mut mouse), [9, 0, 0]);
    assert_eq!(command(&mut mouse, 0xf3), [0xfa]);
    mouse.mouse_event(MouseEvent::FocusLost);
    assert_eq!(command(&mut mouse, 100), [0xfa]);
    mouse.advance(Duration::from_millis(20));
    assert_eq!(drain(&mut mouse), [8, 0, 0]);
    button(&mut mouse, MouseButton::Middle, ElementState::Pressed);
    mouse.advance(Duration::from_millis(30));
    assert_eq!(drain(&mut mouse), [12, 0, 0]);
    mouse.disconnect_mouse();
    mouse.advance(Duration::from_millis(40));
    assert_eq!(drain(&mut mouse), [8, 0, 0]);
    assert_eq!(
        command(&mut mouse, 0xf2),
        [0xfa, 0],
        "firmware outlives the host sender"
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn a_command_cancelling_a_blocked_release_does_not_leave_the_guest_button_held() {
    let mut mouse = mouse();
    assert_eq!(command(&mut mouse, 0xf4), [0xfa]);
    button(&mut mouse, MouseButton::Left, ElementState::Pressed);
    mouse.advance(Duration::from_millis(10));
    assert_eq!(drain(&mut mouse), [9, 0, 0]);
    mouse.event(key(KeyCode::KeyA, true)); // Occupy the shared latch with keyboard output.
    mouse.mouse_event(MouseEvent::FocusLost);
    mouse.advance(Duration::from_millis(20)); // Release packet cannot publish its header.
    mouse.mouse_command(0xf3); // Cancel that packet to start a parameter exchange.
    assert_eq!(mouse.status() & AUXILIARY_OUTPUT, 0);
    assert_eq!(mouse.read_data(), 0x1e);
    assert_eq!(drain(&mut mouse), [0xfa]);
    assert_eq!(command(&mut mouse, 100), [0xfa]);
    mouse.advance(Duration::from_millis(30));
    assert_eq!(
        drain(&mut mouse),
        [8, 0, 0],
        "only transmitted reports advance guest button state"
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn resend_preserves_a_blocked_release_and_motion_accumulated_during_the_replay() {
    let mut mouse = mouse();
    assert_eq!(command(&mut mouse, 0xf4), [0xfa]);
    button(&mut mouse, MouseButton::Left, ElementState::Pressed);
    mouse.advance(Duration::from_millis(10));
    assert_eq!(drain(&mut mouse), [9, 0, 0]);
    mouse.event(key(KeyCode::KeyA, true));
    mouse.mouse_event(MouseEvent::FocusLost);
    motion(&mut mouse, 3.0, 0.0);
    mouse.advance(Duration::from_millis(20));
    mouse.mouse_command(0xfe);
    motion(&mut mouse, 2.0, 0.0);
    assert_eq!(mouse.read_data(), 0x1e);
    assert_eq!(drain(&mut mouse), [9, 0, 0, 8, 3, 0]);
    mouse.advance(Duration::from_millis(30));
    assert_eq!(drain(&mut mouse), [8, 2, 0]);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn a_delayed_report_header_starts_a_new_sample_interval_when_published() {
    let mut mouse = mouse();
    assert_eq!(command(&mut mouse, 0xf4), [0xfa]);
    mouse.event(key(KeyCode::KeyA, true));
    button(&mut mouse, MouseButton::Left, ElementState::Pressed);
    mouse.advance(Duration::from_millis(10));
    button(&mut mouse, MouseButton::Left, ElementState::Released);
    mouse.advance(Duration::from_millis(100));
    assert_eq!(mouse.read_data(), 0x1e);
    assert_eq!(drain(&mut mouse), [9, 0, 0]);
    mouse.advance(Duration::from_millis(109));
    assert!(drain(&mut mouse).is_empty());
    mouse.advance(Duration::from_millis(110));
    assert_eq!(drain(&mut mouse), [8, 0, 0]);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn read_data_includes_unscaled_counters_from_a_report_blocked_before_its_header() {
    let mut mouse = mouse();
    for byte in [0xe7, 0xf4] {
        assert_eq!(command(&mut mouse, byte), [0xfa]);
    }
    mouse.event(key(KeyCode::KeyA, true));
    motion(&mut mouse, 4.0, -2.0);
    mouse.advance(Duration::from_millis(10));
    mouse.mouse_command(0xeb);
    assert_eq!(mouse.read_data(), 0x1e);
    assert_eq!(drain(&mut mouse), [0xfa, 8, 4, 2]);
}
