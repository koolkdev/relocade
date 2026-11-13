mod common;

use common::write;
use relocade_adlib::{ADDRESS_STATUS_PORT, DATA_PORT};
use relocade_clock_test_support::ManualClock;
use std::time::Duration;

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn conventional_detection_needs_no_audio_output() {
    let clock = ManualClock::new();
    let (io, _) = common::hardware(clock.clock());
    write(&io, 4, 0x60); // Mask and stop both timers.
    write(&io, 4, 0x80); // Clear flags.
    assert_eq!(io.read_port(ADDRESS_STATUS_PORT, 1), 0x06);
    write(&io, 2, 0xff);
    write(&io, 4, 0x21); // Start timer 1; mask timer 2.
    clock.advance_by(Duration::from_micros(79));
    assert_eq!(io.read_port(ADDRESS_STATUS_PORT, 1), 0x06);
    clock.advance_by(Duration::from_micros(1));
    assert_eq!(io.read_port(ADDRESS_STATUS_PORT, 1), 0xc6);
    // Reading the latched flag does not acknowledge it.
    assert_eq!(io.read_port(ADDRESS_STATUS_PORT, 1), 0xc6);
    write(&io, 4, 0x60);
    write(&io, 4, 0x80);
    clock.advance_by(Duration::from_secs(1));
    assert_eq!(io.read_port(ADDRESS_STATUS_PORT, 1), 0x06);
    assert_eq!(io.read_port(DATA_PORT, 1), 0xff);
    assert_eq!(io.read_port(ADDRESS_STATUS_PORT, 2), 0xffff);
    assert_eq!(io.read_port(ADDRESS_STATUS_PORT + 2, 1), 0xff);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn reset_and_mask_leave_running_timer_phase_intact() {
    let clock = ManualClock::new();
    let (io, _) = common::hardware(clock.clock());
    write(&io, 3, 0xff);
    write(&io, 4, 2);
    clock.advance_by(Duration::from_micros(319));
    assert_eq!(io.read_port(ADDRESS_STATUS_PORT, 1), 0x06);
    clock.advance_by(Duration::from_micros(1));
    assert_eq!(io.read_port(ADDRESS_STATUS_PORT, 1), 0xa6);
    write(&io, 4, 0xff); // IRQ reset ignores the mask/start bits.
    assert_eq!(io.read_port(ADDRESS_STATUS_PORT, 1), 0x06);
    clock.advance_by(Duration::from_micros(320));
    assert_eq!(io.read_port(ADDRESS_STATUS_PORT, 1), 0xa6);
    write(&io, 4, 0x22); // Masking clears the flag but keeps counting.
    clock.advance_by(Duration::from_micros(500));
    assert_eq!(io.read_port(ADDRESS_STATUS_PORT, 1), 0x06);
    write(&io, 4, 2); // Unmask; next overflow is still at 1280 us.
    clock.advance_by(Duration::from_micros(139));
    assert_eq!(io.read_port(ADDRESS_STATUS_PORT, 1), 0x06);
    clock.advance_by(Duration::from_micros(1));
    assert_eq!(io.read_port(ADDRESS_STATUS_PORT, 1), 0xa6);
    write(&io, 4, 0); // Stopping does not acknowledge the latched flag.
    assert_eq!(io.read_port(ADDRESS_STATUS_PORT, 1), 0xa6);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn running_reload_changes_apply_after_the_current_count() {
    let clock = ManualClock::new();
    let (io, _) = common::hardware(clock.clock());
    write(&io, 2, 0xfe); // First overflow at 160 us.
    write(&io, 4, 1);
    clock.advance_by(Duration::from_micros(80));
    write(&io, 2, 0xff); // Subsequent periods become 80 us.
    io.write_port(ADDRESS_STATUS_PORT, 2, 4); // Wide writes are ignored.
    io.write_port(DATA_PORT, 4, 0);
    write(&io, 4, 1); // Repeating START must not restart the count.
    clock.advance_by(Duration::from_micros(79));
    assert_eq!(io.read_port(ADDRESS_STATUS_PORT, 1), 0x06);
    clock.advance_by(Duration::from_micros(1));
    assert_eq!(io.read_port(ADDRESS_STATUS_PORT, 1), 0xc6);
    clock.advance_by(Duration::from_secs(10));
    write(&io, 4, 0x80);
    clock.advance_by(Duration::from_micros(79));
    assert_eq!(io.read_port(ADDRESS_STATUS_PORT, 1), 0x06);
    clock.advance_by(Duration::from_micros(1));
    assert_eq!(io.read_port(ADDRESS_STATUS_PORT, 1), 0xc6);
}
