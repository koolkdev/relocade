mod common;

use relocade_machine::Device;
use relocade_pc_speaker::{Speaker, SPEAKER_CONTROL_PORT};
use relocade_pit::{CHANNEL_2_PORT, CONTROL_PORT as PIT_CONTROL_PORT, INPUT_HZ};
use std::{rc::Rc, time::Duration};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

fn clocks(count: u64) -> Duration {
    Duration::from_nanos((u128::from(count) * 1_000_000_000).div_ceil(INPUT_HZ.into()) as u64)
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn ports_expose_raw_output_and_direct_speaker_drive() {
    let clock = Rc::new(common::ManualClock::default());
    let mut speaker = Speaker::with_clock(clock.clone());
    speaker.write_port(PIT_CONTROL_PORT, 1, 0xb6);
    speaker.write_port(CHANNEL_2_PORT, 1, 4);
    speaker.write_port(CHANNEL_2_PORT, 1, 0);
    speaker.write_port(SPEAKER_CONTROL_PORT, 1, 1); // Run the timer with speaker drive disabled.
    clock.0.set(clocks(1));
    assert_eq!(speaker.read_port(SPEAKER_CONTROL_PORT, 1), 0x21);
    assert!(!speaker.output());
    speaker.write_port(SPEAKER_CONTROL_PORT, 1, 3);
    assert!(speaker.output());
    clock.0.set(clocks(3));
    assert_eq!(speaker.read_port(SPEAKER_CONTROL_PORT, 1), 3);
    assert!(!speaker.output());
    speaker.write_port(SPEAKER_CONTROL_PORT, 1, 2); // Gate low forces OUT high, enabling direct drive.
    assert_eq!(speaker.read_port(SPEAKER_CONTROL_PORT, 1), 0x22);
    assert!(speaker.output());
    speaker.write_port(SPEAKER_CONTROL_PORT, 1, 0);
    assert!(!speaker.output());
    assert_eq!(speaker.read_port(SPEAKER_CONTROL_PORT, 1), 0x20);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn channel_selection_readback_and_transfer_widths() {
    for command in [0x30, 0x70, 0xc2, 0xc4, 0xc9] {
        let clock = Rc::new(common::ManualClock::default());
        let mut speaker = Speaker::with_clock(clock.clone());
        speaker.write_port(PIT_CONTROL_PORT, 1, 0xb6);
        speaker.write_port(CHANNEL_2_PORT, 1, 5);
        speaker.write_port(CHANNEL_2_PORT, 1, 0);
        speaker.write_port(SPEAKER_CONTROL_PORT, 1, 3);
        clock.0.set(clocks(1));
        speaker.write_port(PIT_CONTROL_PORT, 1, command); // Other channels / reserved read-back.
        speaker.write_port(PIT_CONTROL_PORT, 2, 0xb0);
        speaker.write_port(CHANNEL_2_PORT, 4, 99);
        speaker.write_port(SPEAKER_CONTROL_PORT, 2, 0);
        clock.0.set(clocks(5)); // Count and status now differ from the ignored latch.
        speaker.write_port(PIT_CONTROL_PORT, 1, 0xc8); // Latch both count and status for channel 2.
        clock.0.set(clocks(6));
        assert_eq!(speaker.read_port(CHANNEL_2_PORT, 1), 0x36);
        assert_eq!(speaker.read_port(CHANNEL_2_PORT, 1), 2);
        assert_eq!(speaker.read_port(CHANNEL_2_PORT, 1), 0);
        speaker.write_port(PIT_CONTROL_PORT, 1, 0xe8);
        assert_eq!(speaker.read_port(CHANNEL_2_PORT, 1), 0xb6);
        assert_eq!(speaker.read_port(CHANNEL_2_PORT, 2), u32::MAX);
        assert_eq!(speaker.read_port(PIT_CONTROL_PORT, 1), u32::MAX);
        assert_eq!(speaker.read_port(SPEAKER_CONTROL_PORT, 1), 0x23);
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn frequent_reads_do_not_discard_fractional_clocks() {
    let clock = Rc::new(common::ManualClock::default());
    let mut speaker = Speaker::with_clock(clock.clone());
    speaker.write_port(PIT_CONTROL_PORT, 1, 0xb4);
    speaker.write_port(CHANNEL_2_PORT, 1, 100);
    speaker.write_port(CHANNEL_2_PORT, 1, 0);
    speaker.write_port(SPEAKER_CONTROL_PORT, 1, 1);
    for nanos in (100..=10_000).step_by(100) {
        clock.0.set(Duration::from_nanos(nanos));
        speaker.read_port(SPEAKER_CONTROL_PORT, 1);
    }
    speaker.write_port(PIT_CONTROL_PORT, 1, 0x80); // 11 input clocks: load plus 10 decrements.
    assert_eq!(speaker.read_port(CHANNEL_2_PORT, 1), 90);
    assert_eq!(speaker.read_port(CHANNEL_2_PORT, 1), 0);
}
