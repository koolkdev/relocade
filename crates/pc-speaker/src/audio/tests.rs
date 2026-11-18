#[path = "../../tests/common/mod.rs"]
mod common;

use crate::{EventRecorder, SampleGenerator, Speaker, SPEAKER_CONTROL_PORT};
use relocade_clock_test_support::ManualClock;
use relocade_frontend::audio::AudioTiming;
use relocade_machine::{IoBus, Timestamp};
use relocade_pit::{CHANNEL_2_PORT, CONTROL_PORT as PIT_CONTROL_PORT};
use std::time::Duration;

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

struct Playback {
    recorder: EventRecorder,
    source: SampleGenerator,
    io: IoBus,
    clock: ManualClock,
    timing: AudioTiming,
    frames: u64,
}

fn program(io: &IoBus) {
    io.write_port(PIT_CONTROL_PORT, 1, 0xb6);
    io.write_port(CHANNEL_2_PORT, 1, 1193 & 0xff);
    io.write_port(CHANNEL_2_PORT, 1, 1193 >> 8);
    io.write_port(SPEAKER_CONTROL_PORT, 1, 3);
}

impl Playback {
    fn new(rate: u64) -> Self {
        let clock = ManualClock::new();
        let (io, speaker) = common::hardware(clock.guest_clock());
        let timing =
            AudioTiming::new(clock.clock().now(), clock.clock().tick_rate(), rate as u32).unwrap();
        let (recorder, source) = speaker.borrow().audio_source(timing);
        Self {
            timing,
            recorder,
            source,
            io,
            clock,
            frames: 0,
        }
    }

    fn at_frame(&self, frame: u64) -> Timestamp {
        self.timing.deadline_for_frame(frame)
    }

    fn collect(&mut self, frames: usize) -> Vec<f32> {
        self.clock
            .advance_to(self.at_frame(self.frames + frames as u64));
        let mut output = vec![0.0; frames];
        self.source.generate_samples(
            self.recorder.take_events(),
            self.frames..self.frames + frames as u64,
            &mut output,
        );
        self.frames += frames as u64;
        output
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn tone_continues_without_guest_updates_at_multiple_output_rates() {
    for rate in [44_100, 48_000, 96_000] {
        let mut playback = Playback::new(rate);
        program(&playback.io);
        let samples = playback.collect(rate as usize);
        assert!(samples.iter().all(|sample| (0.0..=1.0).contains(sample)));
        let cycles = samples
            .windows(2)
            .filter(|pair| pair[0] < 0.5 && pair[1] >= 0.5)
            .count();
        // One second of samples; PC input clock / divisor gives 1000.153 Hz.
        assert!((cycles as f64 - 1000.153).abs() < 0.5, "{rate}: {cycles}");
        assert!(samples.iter().any(|&sample| sample > 0.0 && sample < 1.0));
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn controls_mute_and_resume_at_sample_boundaries() {
    let mut playback = Playback::new(48_000);
    assert_eq!(playback.collect(128), vec![0.0; 128]);
    program(&playback.io);
    assert!(playback.collect(512).iter().any(|&sample| sample > 0.0));
    playback.io.write_port(SPEAKER_CONTROL_PORT, 1, 1);
    assert_eq!(playback.collect(128), vec![0.0; 128]);
    // With GATE2 low and data enabled, direct drive remains high.
    playback.io.write_port(SPEAKER_CONTROL_PORT, 1, 2);
    assert_eq!(playback.collect(128), vec![1.0; 128]);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn reads_and_latches_preserve_playback_phase() {
    let mut reference = Playback::new(44_100);
    let mut playback = Playback::new(44_100);
    program(&reference.io);
    program(&playback.io);
    for _ in 0..8 {
        assert_eq!(playback.collect(128), reference.collect(128));
        let io = &playback.io;
        io.read_port(SPEAKER_CONTROL_PORT, 1);
        io.write_port(PIT_CONTROL_PORT, 1, 0x80);
        io.read_port(CHANNEL_2_PORT, 1);
        io.read_port(CHANNEL_2_PORT, 1);
        io.write_port(SPEAKER_CONTROL_PORT, 1, 3);
    }
    // Hardware can advance after programming but before playback takes the update.
    program(&reference.io);
    program(&playback.io);
    playback.clock.advance_by(Duration::from_micros(200));
    reference.clock.advance_by(Duration::from_micros(200));
    playback.io.write_port(PIT_CONTROL_PORT, 1, 0x80);
    playback.io.read_port(CHANNEL_2_PORT, 1);
    playback.io.read_port(CHANNEL_2_PORT, 1);
    playback.io.write_port(SPEAKER_CONTROL_PORT, 1, 3);
    assert_eq!(playback.collect(512), reference.collect(512));
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn timed_controls_and_pit_writes_are_retained_within_samples() {
    let mut playback = Playback::new(1_000);
    playback.io.write_port(PIT_CONTROL_PORT, 1, 0xb6);
    playback.clock.advance_by(Duration::from_micros(2_250));
    playback.io.write_port(SPEAKER_CONTROL_PORT, 1, 2);
    playback.clock.advance_by(Duration::from_micros(2_500));
    // Unsupported mode 0 drives OUT2 low without changing the speaker bits.
    playback.io.write_port(PIT_CONTROL_PORT, 1, 0xb0);
    playback.clock.advance_by(Duration::from_micros(3_250));
    playback.io.write_port(PIT_CONTROL_PORT, 1, 0xb6);
    // Equal-timestamp changes are applied in write order.
    playback.io.write_port(SPEAKER_CONTROL_PORT, 1, 0);
    playback.io.write_port(SPEAKER_CONTROL_PORT, 1, 2);
    let expected = [0.0, 0.0, 0.75, 1.0, 0.75, 0.0, 0.0, 0.0, 1.0, 1.0];
    for (sample, expected) in playback.collect(10).iter().zip(expected) {
        assert!((sample - expected).abs() < 1e-6, "{sample}: {expected}");
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn fractional_pit_initialization_and_controls_share_the_audio_phase() {
    use relocade_machine::GuestTimeDriver;
    use std::{
        future::Future,
        num::NonZeroU64,
        pin::pin,
        task::{Context, Waker},
    };

    let driver = GuestTimeDriver::new(NonZeroU64::new(3_000_000_000).unwrap());
    let clock = driver.guest_clock();
    let mut cx = Context::from_waker(Waker::noop());
    let mut initial = pin!(clock.sleep_until(Timestamp::from_ticks(1)));
    assert!(initial.as_mut().poll(&mut cx).is_pending());
    assert!(driver.advance_to_next_deadline());
    let (io, speaker) = common::hardware(clock.clone());
    let start = clock.now();
    let timing = AudioTiming::new(start, clock.tick_rate(), 1_000_000_000).unwrap();
    let (mut recorder, mut source) = speaker.borrow().audio_source(timing);
    io.write_port(PIT_CONTROL_PORT, 1, 0xb6);

    let mut control = pin!(clock.sleep_until(Timestamp::from_ticks(6)));
    assert!(control.as_mut().poll(&mut cx).is_pending());
    assert!(driver.advance_to_next_deadline());
    // The change occurs at native tick 6, two thirds through the second frame.
    io.write_port(SPEAKER_CONTROL_PORT, 1, 2);
    let mut end = pin!(clock.sleep_until(Timestamp::from_ticks(10)));
    assert!(end.as_mut().poll(&mut cx).is_pending());
    assert!(driver.advance_to_next_deadline());
    let mut samples = [0.0; 3];
    source.generate_samples(recorder.take_events(), 0..3, &mut samples);
    assert_eq!(samples, [0.0, 1.0 / 3.0, 1.0]);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn block_partitioning_preserves_tone_and_mid_frame_controls() {
    let generate = |batch: usize| {
        let mut playback = Playback::new(44_100);
        program(&playback.io);
        playback.clock.advance_by(Duration::from_micros(2_250));
        playback.io.write_port(SPEAKER_CONTROL_PORT, 1, 1);
        playback.clock.advance_by(Duration::from_micros(2_500));
        playback.io.write_port(SPEAKER_CONTROL_PORT, 1, 3);
        let mut output = Vec::new();
        while output.len() < 1024 {
            output.extend(playback.collect(batch.min(1024 - output.len())));
        }
        output
    };
    let expected = generate(1024);
    for batch in [1, 7, 128, 256] {
        assert_eq!(generate(batch), expected);
    }
}
