use relocade_clock_test_support::ManualClock;
#[path = "../../tests/common/mod.rs"]
mod common;

use crate::{Speaker, PORT_RANGE, SPEAKER_CONTROL_PORT};
use async_channel::Receiver;

use relocade_machine::{Device, IoBus};
use relocade_pit::{CHANNEL_2_PORT, CONTROL_PORT as PIT_CONTROL_PORT};
use std::{
    cell::RefCell,
    future::Future,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll, Waker},
    time::Duration,
};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

struct Playback {
    speaker: Rc<RefCell<Speaker>>,
    io: IoBus,
    clock: ManualClock,
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

fn program(io: &IoBus) {
    io.write_port(PIT_CONTROL_PORT, 1, 0xb6);
    io.write_port(CHANNEL_2_PORT, 1, 1193 & 0xff);
    io.write_port(CHANNEL_2_PORT, 1, 1193 >> 8);
    io.write_port(SPEAKER_CONTROL_PORT, 1, 3);
}

impl Playback {
    fn new(config: cpal::StreamConfig) -> Self {
        let (samples, output) = async_channel::bounded(1);
        let clock = ManualClock::default();
        let (io, speaker) = common::hardware(clock.clock());
        speaker.borrow_mut().attach_audio(config, samples).unwrap();
        Self {
            task: Box::pin(Speaker::run(speaker.clone())),
            speaker,
            io,
            clock,
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
        if self.output.is_empty() {
            self.clock.advance_to(self.clock.next_deadline().unwrap());
            assert!(self.poll().is_pending());
        }
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
        program(&playback.io);
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
    program(&reference.io);
    let expected = reference.collect(8000);
    for batch in [1, 7, 128] {
        let mut playback = Playback::new(config(44_100, 1, batch));
        program(&playback.io);
        assert_eq!(playback.collect(expected.len()), expected, "batch {batch}");
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn controls_mute_and_resume_at_buffer_boundaries() {
    let mut playback = Playback::new(config(48_000, 1, 128));
    assert!(playback.next_buffer().iter().all(|&sample| sample == 0.0));
    program(&playback.io);
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
fn reads_and_latches_preserve_playback_phase() {
    let mut reference = Playback::new(config(44_100, 1, 128));
    let mut playback = Playback::new(config(44_100, 1, 128));
    program(&reference.io);
    program(&playback.io);
    for _ in 0..8 {
        assert_eq!(playback.next_buffer(), reference.next_buffer());
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
fn clock_bounds_production_and_output_close_ends_the_activity() {
    for enabled in [false, true] {
        let mut playback = Playback::new(config(48_000, 2, 4096));
        if enabled {
            program(&playback.io);
        }
        assert!(playback.poll().is_pending());
        assert!(playback.poll().is_pending());
        assert!(playback.output.is_empty());
        playback
            .clock
            .advance_to(playback.clock.next_deadline().unwrap());
        assert!(playback.poll().is_pending());
        assert_eq!(playback.output.len(), 1);
        assert_eq!(playback.output.try_recv().unwrap().len(), 256 * 2);
        playback.output.close();
        assert!(playback.poll().is_ready());
    }

    let playback = Playback::new(config(48_000, 1, 128));
    drop(playback.task);
    drop(playback.speaker);
    drop(playback.io);
    assert!(playback.output.is_closed());
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn timed_controls_and_pit_writes_are_retained_within_samples() {
    let mut playback = Playback::new(config(1_000, 2, 10));
    let start = playback.clock.clock().now();
    playback.io.write_port(PIT_CONTROL_PORT, 1, 0xb6);
    assert!(playback.poll().is_pending());
    playback
        .clock
        .advance_to(start + Duration::from_micros(2_250));
    playback.io.write_port(SPEAKER_CONTROL_PORT, 1, 2);
    playback
        .clock
        .advance_to(start + Duration::from_micros(4_750));
    // Unsupported mode 0 drives OUT2 low without changing the speaker bits.
    playback.io.write_port(PIT_CONTROL_PORT, 1, 0xb0);
    playback.clock.advance_to(start + Duration::from_millis(8));
    playback.io.write_port(PIT_CONTROL_PORT, 1, 0xb6);
    // Equal-timestamp changes are applied in write order.
    playback.io.write_port(SPEAKER_CONTROL_PORT, 1, 0);
    playback.io.write_port(SPEAKER_CONTROL_PORT, 1, 2);
    assert!(playback.poll().is_pending());
    assert!(playback.output.is_empty());
    let buffer = playback.next_buffer();
    let expected = [0.0, 0.0, 0.15, 0.2, 0.15, 0.0, 0.0, 0.0, 0.2, 0.2];
    for (frame, expected) in buffer.as_chunks::<2>().0.iter().zip(expected) {
        assert!((frame[0] - expected).abs() < 1e-6, "{frame:?}: {expected}");
        assert_eq!(frame[0], frame[1]);
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn full_output_retains_recent_pcm_and_drains_control_changes() {
    let mut playback = Playback::new(config(1_000, 1, 10));
    assert!(playback.poll().is_pending());
    playback
        .clock
        .advance_to(playback.clock.next_deadline().unwrap());
    assert!(playback.poll().is_pending());
    assert_eq!(playback.output.len(), 1); // Leave the initial silence queued.
    playback.io.write_port(PIT_CONTROL_PORT, 1, 0xb6);
    playback.io.write_port(SPEAKER_CONTROL_PORT, 1, 2);
    playback.clock.advance_by(Duration::from_millis(30));
    assert!(playback.poll().is_pending());
    assert_eq!(playback.output.len(), 1);
    assert_eq!(playback.output.try_recv().unwrap(), vec![0.2; 10]);
    playback.io.write_port(SPEAKER_CONTROL_PORT, 1, 0);
    assert_eq!(playback.next_buffer(), vec![0.0; 10]);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn invalid_configuration_and_unbounded_output_are_rejected() {
    for config in [
        config(0, 1, 128),
        config(48_000, 0, 128),
        config(1_000_000_001, 1, 128),
    ] {
        let (samples, _output) = async_channel::bounded(1);
        let (_io, speaker) = common::hardware(ManualClock::new().clock());
        assert!(speaker.borrow_mut().attach_audio(config, samples).is_err());
    }
    let (samples, _output) = async_channel::unbounded();
    let (_io, speaker) = common::hardware(ManualClock::new().clock());
    assert!(speaker
        .borrow_mut()
        .attach_audio(config(48_000, 1, 128), samples)
        .is_err());
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn attaching_output_wakes_an_already_running_device() {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    struct Woken(AtomicBool);
    impl std::task::Wake for Woken {
        fn wake(self: Arc<Self>) {
            self.0.store(true, Ordering::Relaxed);
        }
    }

    let clock = ManualClock::new();
    let (io, speaker) = common::hardware(clock.clock());
    let mut task = std::pin::pin!(Speaker::run(speaker.clone()));
    let woken = Arc::new(Woken(AtomicBool::new(false)));
    let waker = Waker::from(woken.clone());
    let mut cx = Context::from_waker(&waker);
    assert!(task.as_mut().poll(&mut cx).is_pending());
    program(&io);
    let (samples, output) = async_channel::bounded(1);
    speaker
        .borrow_mut()
        .attach_audio(config(48_000, 1, 128), samples.clone())
        .unwrap();
    assert!(woken.0.load(Ordering::Relaxed));
    assert!(task.as_mut().poll(&mut cx).is_pending());
    assert!(output.is_empty());
    clock.advance_to(clock.next_deadline().unwrap());
    assert!(task.as_mut().poll(&mut cx).is_pending());
    assert!(output
        .try_recv()
        .unwrap()
        .iter()
        .any(|&sample| sample > 0.0));
    assert!(speaker
        .borrow_mut()
        .attach_audio(config(48_000, 1, 128), samples)
        .is_err());
    output.close();
    assert!(task.as_mut().poll(&mut cx).is_ready());
}
