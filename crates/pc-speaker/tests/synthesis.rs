mod common;

use relocade_frontend::audio::AudioTiming;
use relocade_machine::{GuestTimeDriver, Timestamp};
use relocade_pc_speaker::{Speaker, SPEAKER_CONTROL_PORT};
use relocade_pit::{CHANNEL_2_PORT, CONTROL_PORT, INPUT_HZ};
use std::{
    future::Future,
    num::NonZeroU64,
    pin::pin,
    task::{Context, Waker},
};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

fn advance_to(driver: &GuestTimeDriver, ticks: u64) -> Timestamp {
    let clock = driver.clock();
    let deadline = Timestamp::from_ticks(ticks);
    let mut sleep = pin!(clock.sleep_until(deadline));
    assert!(sleep
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
        .is_pending());
    assert!(driver.advance_to_next_deadline());
    assert_eq!(clock.now(), deadline);
    deadline
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn fine_frame_grid_preserves_pitch_and_level() {
    let driver = GuestTimeDriver::new(NonZeroU64::new(3_000_000_000).unwrap());
    let (io, speaker) = common::hardware(driver.guest_clock());
    io.write_port(CONTROL_PORT, 1, 0xb6);
    io.write_port(CHANNEL_2_PORT, 1, 2);
    io.write_port(CHANNEL_2_PORT, 1, 0);
    io.write_port(SPEAKER_CONTROL_PORT, 1, 3);
    let timing = AudioTiming::new(
        driver.clock().now(),
        driver.clock().tick_rate(),
        750_000_000,
    )
    .unwrap();
    let (mut recorder, mut source) = speaker.borrow().audio_source(timing);
    advance_to(&driver, 40_000);

    // Four native ticks span 4/3 ns. Rounding each elapsed span separately
    // would advance the tone at only 3/4 of its programmed frequency.
    let mut samples = vec![0.0; 10_000];
    source.generate_samples(recorder.take_events(), 0..10_000, &mut samples);
    // Carrying time into a later sample must not amplify that sample.
    assert!(samples.iter().all(|sample| (0.0..=1.0).contains(sample)));
    let edges: Vec<_> = samples
        .windows(2)
        .enumerate()
        .filter_map(|(index, pair)| (pair[0] < 0.5 && pair[1] >= 0.5).then_some(index + 1))
        .collect();
    assert!(edges.len() > 2);
    let period = (edges.last().unwrap() - edges[0]) as f64 / (edges.len() - 1) as f64;
    // 750 MHz sampling, with the PIT running at its input frequency / 2.
    let expected = 750_000_000.0 * 2.0 / INPUT_HZ as f64;
    assert!((period - expected).abs() < 1.0, "{period}: {expected}");
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn offset_sample_epoch_preserves_level_across_a_pit_write() {
    let driver = GuestTimeDriver::new(NonZeroU64::new(3_000_000_000).unwrap());
    advance_to(&driver, 1);
    let (io, speaker) = common::hardware(driver.guest_clock());
    io.write_port(CONTROL_PORT, 1, 0xb6);
    io.write_port(SPEAKER_CONTROL_PORT, 1, 2);
    // The shared audio origin can differ from PIT initialization.
    let start = advance_to(&driver, 2);
    let timing = AudioTiming::new(start, driver.clock().tick_rate(), 1_000_000_000).unwrap();
    let (mut recorder, mut source) = speaker.borrow().audio_source(timing);
    advance_to(&driver, 4);
    io.write_port(CONTROL_PORT, 1, 0xb6); // OUT2 stays high with GATE2 low.
    advance_to(&driver, 5);
    let mut output = [0.0];
    source.generate_samples(recorder.take_events(), 0..1, &mut output);
    assert_eq!(output, [1.0]);
}
