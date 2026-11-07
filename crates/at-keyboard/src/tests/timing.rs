use super::*;

fn micros(value: u64) -> Duration {
    Duration::from_micros(value)
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn serial_bytes_arrive_at_deadlines_despite_other_key_events() {
    let (mut keyboard, mut pic) = controller();
    keyboard.queue_event(key(KeyCode::KeyA, true));
    keyboard.settle();
    keyboard.advance(micros(400));
    keyboard.queue_event(key(KeyCode::KeyB, true));
    keyboard.settle();
    keyboard.advance(micros(879));
    assert_eq!(keyboard.status() & OUTPUT_BUFFER_FULL, 0);
    assert_eq!(poll(&mut pic), Poll::Pending);
    keyboard.advance(micros(880));
    assert_eq!(poll(&mut pic), Poll::Ready(9));
    assert_eq!(keyboard.read_unpolled(), 0x1e);
    keyboard.settle();
    keyboard.advance(micros(1759));
    assert_eq!(keyboard.status() & OUTPUT_BUFFER_FULL, 0);
    assert_eq!(
        keyboard.read_unpolled(),
        0x1e,
        "latch retained between frames"
    );
    keyboard.advance(micros(1760));
    assert_eq!(keyboard.read_unpolled(), 0x30);
    assert_eq!(
        poll(&mut pic),
        Poll::Pending,
        "PIC still owns interrupt completion"
    );
    pic.write_port(relocade_pic::COMMAND_PORT, 1, 0x20);
    assert_eq!(poll(&mut pic), Poll::Ready(9));
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn inhibition_restarts_an_unfinished_byte_and_full_output_holds_the_next() {
    let (mut keyboard, _) = controller();
    keyboard.queue_event(key(KeyCode::KeyA, true));
    keyboard.settle();
    keyboard.advance(micros(400));
    keyboard.command(DISABLE_INTERFACE);
    keyboard.advance(micros(2000));
    assert_eq!(keyboard.status() & OUTPUT_BUFFER_FULL, 0);
    keyboard.write_unpolled(STATUS_COMMAND_PORT, ENABLE_INTERFACE);
    keyboard.settle();
    keyboard.advance(micros(2879));
    assert_eq!(keyboard.status() & OUTPUT_BUFFER_FULL, 0);
    keyboard.advance(micros(2880));
    keyboard.queue_event(key(KeyCode::KeyB, true));
    keyboard.settle();
    keyboard.advance(micros(10_000));
    assert_eq!(
        keyboard.read_unpolled(),
        0x1e,
        "unread output is never replaced"
    );
    keyboard.settle();
    keyboard.advance(micros(10_879));
    assert_eq!(keyboard.status() & OUTPUT_BUFFER_FULL, 0);
    keyboard.advance(micros(10_880));
    assert_eq!(keyboard.read_unpolled(), 0x30);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn translated_break_prefix_still_takes_a_wire_frame() {
    let (mut keyboard, _) = controller();
    keyboard.event(key(KeyCode::KeyA, true));
    assert_eq!(keyboard.read_unpolled(), 0x1e);
    let start = keyboard.now();
    keyboard.queue_event(key(KeyCode::KeyA, false));
    keyboard.settle();
    keyboard.advance(start + micros(880));
    assert_eq!(
        keyboard.status() & OUTPUT_BUFFER_FULL,
        0,
        "F0 is consumed by translation"
    );
    keyboard.advance(start + micros(1759));
    assert_eq!(keyboard.status() & OUTPUT_BUFFER_FULL, 0);
    keyboard.advance(start + micros(1760));
    assert_eq!(keyboard.read_unpolled(), 0x9e);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn host_command_replaces_the_wire_transfer_without_losing_queued_scans() {
    let (mut keyboard, _) = controller();
    keyboard.queue_event(key(KeyCode::KeyA, true));
    keyboard.settle();
    keyboard.advance(micros(400));
    keyboard.write_unpolled(DATA_PORT, 0xee);
    keyboard.settle();
    keyboard.advance(micros(1279));
    assert_eq!(keyboard.status() & OUTPUT_BUFFER_FULL, 0);
    keyboard.advance(micros(1280));
    assert_eq!(
        keyboard.status() & OUTPUT_BUFFER_FULL,
        0,
        "host command has just arrived"
    );
    keyboard.advance(micros(2159));
    assert_eq!(keyboard.status() & OUTPUT_BUFFER_FULL, 0);
    keyboard.advance(micros(2160));
    assert_eq!(keyboard.read_unpolled(), 0xee);
    keyboard.settle();
    keyboard.advance(micros(3040));
    assert_eq!(keyboard.read_unpolled(), 0x1e);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn reset_waits_for_ack_release_and_self_test_before_rescanning_held_keys() {
    let (mut keyboard, _) = controller();
    keyboard.command(0x60);
    keyboard.data(5); // Raw set 2 output, without controller translation.
    keyboard.data(0xff);
    keyboard.advance(keyboard.now() + Duration::from_millis(500));
    assert_eq!(
        keyboard.read_unpolled(),
        0xfa,
        "unread ACK prevents reset from starting"
    );
    keyboard.settle();
    let released = keyboard.now();
    keyboard.advance(released + micros(200));
    keyboard.write_unpolled(STATUS_COMMAND_PORT, DISABLE_INTERFACE);
    keyboard.settle();
    keyboard.advance(released + micros(700));
    keyboard.write_unpolled(STATUS_COMMAND_PORT, ENABLE_INTERFACE);
    keyboard.settle();
    keyboard.advance(released + micros(1199));
    assert_eq!(keyboard.status() & OUTPUT_BUFFER_FULL, 0);
    let started = released + micros(1200);
    keyboard.advance(started); // 500 uninterrupted microseconds start BAT.
    keyboard.queue_event(key(KeyCode::KeyA, true));
    keyboard.settle();
    keyboard.advance(started + Duration::from_millis(50));
    keyboard.data(0xee); // Commands during BAT are ignored.
    keyboard.advance(started + micros(399_999));
    assert_eq!(keyboard.status() & OUTPUT_BUFFER_FULL, 0);
    keyboard.advance(started + micros(400_000));
    keyboard.advance(started + micros(400_879));
    assert_eq!(keyboard.status() & OUTPUT_BUFFER_FULL, 0);
    keyboard.advance(started + micros(400_880));
    assert_eq!(keyboard.read_unpolled(), 0xaa);
    keyboard.settle();
    keyboard.advance(started + micros(401_760));
    assert_eq!(
        keyboard.read_unpolled(),
        0x1c,
        "scanning resumes with the held key"
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn another_command_before_reset_ack_acceptance_preserves_the_old_scan_set() {
    let (mut keyboard, _) = controller();
    keyboard.command(0x60);
    keyboard.data(5);
    keyboard.data(0xf0);
    assert_eq!(drain(&mut keyboard), [0xfa]);
    keyboard.data(1);
    assert_eq!(drain(&mut keyboard), [0xfa]);
    keyboard.data(0xff);
    assert_eq!(keyboard.read_unpolled(), 0xfa);
    keyboard.settle();
    keyboard.write_unpolled(DATA_PORT, 0xee);
    keyboard.settle();
    keyboard.finish_transfers();
    assert_eq!(drain(&mut keyboard), [0xee]);
    keyboard.data(0xf0);
    assert_eq!(drain(&mut keyboard), [0xfa]);
    keyboard.data(0);
    assert_eq!(drain(&mut keyboard), [0xfa, 1]);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn replacing_the_self_test_reply_still_resumes_scanning() {
    let (mut keyboard, _) = controller();
    keyboard.data(0xff);
    assert_eq!(drain(&mut keyboard), [0xfa]);
    keyboard.advance(keyboard.now() + Duration::from_millis(400));
    assert_eq!(
        keyboard.status() & OUTPUT_BUFFER_FULL,
        0,
        "AAh is still in flight"
    );
    keyboard.queue_event(key(KeyCode::KeyA, true));
    keyboard.settle();
    keyboard.write_unpolled(DATA_PORT, 0xee);
    keyboard.settle();
    keyboard.finish_transfers();
    assert_eq!(drain(&mut keyboard), [0xee, 0x1e]);
    keyboard.event(key(KeyCode::KeyA, false));
    assert_eq!(drain(&mut keyboard), [0x9e]);
}
