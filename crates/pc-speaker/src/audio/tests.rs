#[path = "../../tests/common/mod.rs"]
mod common;

use crate::{Speaker, SPEAKER_CONTROL_PORT};
use async_channel::Receiver;
use common::ManualClock;
use relocade_machine::Device;
use relocade_pit::{CHANNEL_2_PORT, CONTROL_PORT as PIT_CONTROL_PORT};
use std::{
    cell::RefCell,
    future::Future,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll, Waker},
};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

struct Playback {
    speaker: Rc<RefCell<Speaker<ManualClock>>>,
    output: Receiver<Vec<f32>>,
    task: Pin<Box<dyn Future<Output = ()>>>,
}

fn config(rate: u32, channels: u16, batch: u32) -> cpal::StreamConfig {
    cpal::StreamConfig {
        sample_rate: rate,
        channels,
        buffer_size: cpal::BufferSize::Fixed(batch),
    }
}

fn program(speaker: &mut impl Device) {
    speaker.write_port(PIT_CONTROL_PORT, 1, 0xb6);
    speaker.write_port(CHANNEL_2_PORT, 1, 1193 & 0xff);
    speaker.write_port(CHANNEL_2_PORT, 1, 1193 >> 8);
    speaker.write_port(SPEAKER_CONTROL_PORT, 1, 3);
}

impl Playback {
    fn new(config: cpal::StreamConfig) -> Self {
        let (samples, output) = async_channel::bounded(1);
        let speaker = Rc::new(RefCell::new(
            Speaker::with_clock(Rc::new(ManualClock::default()))
                .with_audio(config, samples)
                .unwrap(),
        ));
        Self {
            task: Box::pin(Speaker::run(speaker.clone())),
            speaker,
            output,
        }
    }

    fn poll(&mut self) -> Poll<()> {
        self.task
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
    }

    fn next_buffer(&mut self) -> Vec<f32> {
        assert!(self.poll().is_pending());
        self.output.try_recv().unwrap()
    }

    fn collect(&mut self, samples: usize) -> Vec<f32> {
        let mut result = Vec::new();
        while result.len() < samples {
            result.extend(self.next_buffer());
        }
        result.truncate(samples);
        result
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn tone_continues_without_guest_updates_at_multiple_output_rates() {
    for rate in [44_100, 48_000, 96_000] {
        let mut playback = Playback::new(config(rate, 2, 256));
        program(&mut *playback.speaker.borrow_mut());
        let output = playback.collect(rate as usize * 2);
        let mono: Vec<_> = output
            .as_chunks::<2>()
            .0
            .iter()
            .map(|frame| {
                assert_eq!(frame[0], frame[1]);
                assert!((0.0..=0.2).contains(&frame[0]));
                frame[0]
            })
            .collect();
        let cycles = mono
            .windows(2)
            .filter(|pair| pair[0] < 0.1 && pair[1] >= 0.1)
            .count();
        // One second of samples; PC input clock / divisor gives 1000.153 Hz.
        assert!((cycles as f64 - 1000.153).abs() < 0.5, "{rate}: {cycles}");
        assert!(mono.iter().any(|&sample| sample > 0.0 && sample < 0.2));
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn buffer_boundaries_preserve_phase_and_fractional_clock_remainder() {
    let mut reference = Playback::new(config(44_100, 1, 256));
    program(&mut *reference.speaker.borrow_mut());
    let expected = reference.collect(8000);
    for batch in [1, 7, 128] {
        let mut playback = Playback::new(config(44_100, 1, batch));
        program(&mut *playback.speaker.borrow_mut());
        assert_eq!(playback.collect(expected.len()), expected, "batch {batch}");
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn controls_mute_and_resume_at_buffer_boundaries() {
    let mut playback = Playback::new(config(48_000, 1, 128));
    assert!(playback.next_buffer().iter().all(|&sample| sample == 0.0));
    program(&mut *playback.speaker.borrow_mut());
    let tone = playback.collect(512);
    assert!(tone[384..].iter().any(|&sample| sample > 0.0));
    playback
        .speaker
        .borrow_mut()
        .write_port(SPEAKER_CONTROL_PORT, 1, 1);
    let muted = playback.collect(512);
    assert!(muted[384..].iter().all(|&sample| sample == 0.0));
    // With GATE2 low and data enabled, direct drive remains high.
    playback
        .speaker
        .borrow_mut()
        .write_port(SPEAKER_CONTROL_PORT, 1, 2);
    let direct = playback.collect(512);
    assert!(direct[384..].iter().all(|&sample| sample == 0.2));
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn reads_and_latches_do_not_change_playback_or_advance_guest_time() {
    let mut reference = Playback::new(config(44_100, 1, 128));
    let mut playback = Playback::new(config(44_100, 1, 128));
    program(&mut *reference.speaker.borrow_mut());
    program(&mut *playback.speaker.borrow_mut());
    for _ in 0..8 {
        assert_eq!(playback.next_buffer(), reference.next_buffer());
        let mut speaker = playback.speaker.borrow_mut();
        assert_eq!(speaker.read_port(SPEAKER_CONTROL_PORT, 1), 0x23);
        speaker.write_port(PIT_CONTROL_PORT, 1, 0x80);
        // The machine clock has not advanced to the initial divisor load yet.
        assert_eq!(speaker.read_port(CHANNEL_2_PORT, 1), 0);
        assert_eq!(speaker.read_port(CHANNEL_2_PORT, 1), 0);
        speaker.write_port(SPEAKER_CONTROL_PORT, 1, 3);
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn backpressure_bounds_submission_and_output_close_ends_the_activity() {
    for enabled in [false, true] {
        let mut playback = Playback::new(config(48_000, 2, 4096));
        if enabled {
            program(&mut *playback.speaker.borrow_mut());
        }
        assert!(playback.poll().is_pending());
        assert!(playback.poll().is_pending());
        assert_eq!(playback.output.len(), 1);
        assert_eq!(playback.output.try_recv().unwrap().len(), 256 * 2);
        playback.output.close();
        assert!(playback.poll().is_ready());
    }

    let playback = Playback::new(config(48_000, 1, 128));
    drop(playback.task);
    drop(playback.speaker);
    assert!(playback.output.is_closed());
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn invalid_configuration_and_unbounded_output_are_rejected() {
    for config in [config(0, 1, 128), config(48_000, 0, 128)] {
        let (samples, _output) = async_channel::bounded(1);
        assert!(Speaker::with_clock(Rc::new(ManualClock::default()))
            .with_audio(config, samples)
            .is_err());
    }
    let (samples, _output) = async_channel::unbounded();
    assert!(Speaker::with_clock(Rc::new(ManualClock::default()))
        .with_audio(config(48_000, 1, 128), samples)
        .is_err());
}
