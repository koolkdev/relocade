mod common;

use common::write;
use relocade_adlib::{AudioSource, ADDRESS_STATUS_PORT};
use relocade_clock_test_support::ManualClock;
use relocade_machine::{IoBus, Timestamp};
use std::time::Duration;

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

fn time(clock: &ManualClock, frame: usize, rate: u32) -> Timestamp {
    clock.at(Duration::from_nanos(
        (frame as u64 * 1_000_000_000).div_ceil(u64::from(rate)),
    ))
}

fn render_elapsed(
    source: &mut AudioSource,
    clock: &ManualClock,
    io: &IoBus,
    rate: u32,
    samples: &mut Vec<f32>,
) {
    while time(clock, samples.len() + 1, rate) <= clock.clock().now() {
        let start = time(clock, samples.len(), rate);
        let end = time(clock, samples.len() + 1, rate);
        // Polling status alongside output must not advance synthesis twice.
        assert_eq!(io.read_port(ADDRESS_STATUS_PORT, 1), 0xc6);
        samples.push(source.sample(start..end));
    }
}

fn melody(rate: u32, poll_between_writes: bool) -> Vec<f32> {
    let clock = ManualClock::new();
    let (io, adlib) = common::hardware(clock.clock());
    let mut source = adlib.borrow_mut().audio_source(rate).unwrap();
    for (register, value) in [
        (0x20, 0x21),
        (0x23, 0x21),
        (0x40, 0x3f),
        (0x43, 0),
        (0x60, 0xf0),
        (0x63, 0xf0),
        (0x80, 0x0f),
        (0x83, 0x0f),
        (0xa0, 0x98),
        (0x02, 0xff),
        (0x04, 1), // Detection remains live during rendering.
    ] {
        write(&io, register, value);
    }
    let mut samples = Vec::new();
    for (micros, key) in [
        (5_001, 0x31),
        (15_003, 0x11),
        (25_007, 0x31),
        (35_009, 0x11),
    ] {
        clock.advance_to(clock.at(Duration::from_micros(micros)));
        if poll_between_writes {
            render_elapsed(&mut source, &clock, &io, rate, &mut samples);
        }
        write(&io, 0xb0, key);
    }
    clock.advance_to(clock.at(Duration::from_millis(80)));
    render_elapsed(&mut source, &clock, &io, rate, &mut samples);
    samples
}

fn peak(samples: &[f32], rate: u32, start_ms: usize, end_ms: usize) -> f32 {
    samples[start_ms * rate as usize / 1000..end_ms * rate as usize / 1000]
        .iter()
        .map(|sample| sample.abs())
        .fold(0.0, f32::max)
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn short_notes_survive_delayed_mixer_polls_at_both_rates() {
    for rate in [44_100, 48_000] {
        let delayed = melody(rate, false);
        assert_eq!(delayed, melody(rate, true));
        assert_eq!(peak(&delayed, rate, 0, 5), 0.0);
        assert!(peak(&delayed, rate, 7, 14) > 0.05);
        assert!(peak(&delayed, rate, 21, 24) < 0.001);
        assert!(peak(&delayed, rate, 27, 34) > 0.05);
        assert!(peak(&delayed, rate, 55, 80) < 0.001);
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn rhythm_register_keys_the_bass_drum_without_a_melodic_key_on() {
    let clock = ManualClock::new();
    let (io, adlib) = common::hardware(clock.clock());
    let mut source = adlib.borrow_mut().audio_source(48_000).unwrap();
    for (register, value) in [
        (0x30, 0x21),
        (0x33, 0x21),
        (0x50, 0x3f),
        (0x53, 0),
        (0x70, 0xf0),
        (0x73, 0xf0),
        (0x90, 0x0f),
        (0x93, 0x0f),
        (0xa6, 0x98),
        (0xb6, 0x11),
        (0xbd, 0x20),
        (0x02, 0xff),
        (0x04, 1),
    ] {
        write(&io, register, value);
    }
    clock.advance_to(clock.at(Duration::from_millis(20)));
    write(&io, 0xbd, 0x30);
    clock.advance_to(clock.at(Duration::from_millis(40)));
    write(&io, 0xbd, 0x20);
    clock.advance_to(clock.at(Duration::from_millis(80)));
    let mut samples = Vec::new();
    render_elapsed(&mut source, &clock, &io, 48_000, &mut samples);
    // Unkeyed rhythm operators retain the core's small PCM quantization residue.
    assert!(peak(&samples, 48_000, 0, 20) < 0.001);
    assert!(peak(&samples, 48_000, 23, 38) > 0.05);
    assert!(peak(&samples, 48_000, 60, 80) < 0.001);
}
