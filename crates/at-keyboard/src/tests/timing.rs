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
    assert_eq!(keyboard.read_unpolled(), 0xee);
    keyboard.settle();
    keyboard.advance(micros(2160));
    assert_eq!(keyboard.read_unpolled(), 0x1e);
}
