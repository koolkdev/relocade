use super::*;
use relocade_clock_test_support::ManualClock;
use std::{
    future::Future,
    pin::{pin, Pin},
    sync::{Arc, Mutex},
    task::{Context, Poll, Waker},
    time::Duration,
};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

struct Playback {
    clock: ManualClock,
    output: async_channel::Receiver<Vec<f32>>,
    task: Pin<Box<dyn Future<Output = ()>>>,
}

fn config(sample_rate: u32, channels: u16, block_frames: usize) -> Config {
    Config {
        sample_rate,
        channels,
        block_frames,
    }
}

impl Playback {
    fn new(config: Config, register: impl FnOnce(&mut Sources)) -> Self {
        let clock = ManualClock::new();
        let (samples, output) = async_channel::bounded(1);
        let mut audio = Audio::new(clock.clock(), config, samples).unwrap();
        register(&mut audio.sources);
        Self {
            clock,
            output,
            task: Box::pin(audio.run()),
        }
    }

    fn poll(&mut self) -> Poll<()> {
        self.task
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
    }

    fn next_buffer(&mut self) -> Vec<f32> {
        assert!(self.poll().is_pending());
        self.clock.advance_to(self.clock.next_deadline().unwrap());
        assert!(self.poll().is_pending());
        self.output.try_recv().unwrap()
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn block_deadlines_and_frame_ranges_stay_aligned_across_batch_sizes() {
    for batch in [1, 7, 128, 256] {
        let frames = Arc::new(Mutex::new(Vec::new()));
        let recorded = frames.clone();
        let mut playback = Playback::new(config(44_100, 1, batch), |sources| {
            sources.add_source(
                1.0,
                || (),
                move |(), frames, output| {
                    recorded.lock().unwrap().extend(frames);
                    output.fill(0.0);
                },
            );
        });
        let mut end = 0;
        while end < 1024 {
            assert!(playback.poll().is_pending());
            end += batch as u64;
            assert_eq!(
                playback.clock.next_deadline(),
                Some(
                    playback
                        .clock
                        .at(Duration::from_nanos((end * 1_000_000_000).div_ceil(44_100)))
                ),
            );
            playback.next_buffer();
        }
        assert_eq!(*frames.lock().unwrap(), (0..end).collect::<Vec<_>>());
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn frames_remain_distinct_below_one_native_tick() {
    use relocade_machine::GuestTimeDriver;
    use std::num::NonZeroU64;
    let driver = GuestTimeDriver::new(NonZeroU64::new(1000).unwrap());
    let (samples, output) = async_channel::bounded(1);
    let mut audio = Audio::new(driver.clock(), config(4000, 1, 4), samples).unwrap();
    audio.sources.add_source(
        1.0,
        || (),
        |(), frames, output| {
            for (frame, sample) in frames.zip(output) {
                *sample = frame as f32;
            }
        },
    );
    let mut activity = pin!(audio.run());
    let mut cx = Context::from_waker(Waker::noop());
    assert!(activity.as_mut().poll(&mut cx).is_pending());
    assert_eq!(driver.next_deadline().unwrap().ticks(), 1);
    assert!(driver.advance_to_next_deadline());
    assert!(activity.as_mut().poll(&mut cx).is_pending());
    assert_eq!(output.try_recv().unwrap(), [0.0, 1.0, 2.0, 3.0]);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn full_output_keeps_recent_samples_and_closure_releases_sources() {
    let samples = Arc::new(Mutex::new(0));
    let generated = samples.clone();
    let mut playback = Playback::new(config(1000, 1, 10), |sources| {
        sources.add_source(
            1.0,
            || (),
            move |(), _, output| {
                let mut count = generated.lock().unwrap();
                for sample in output {
                    *count += 1;
                    *sample = *count as f32;
                }
            },
        );
    });
    assert!(playback.poll().is_pending());
    playback.clock.advance_by(Duration::from_millis(30));
    assert!(playback.poll().is_pending());
    assert_eq!(*samples.lock().unwrap(), 30);
    assert_eq!(playback.output.len(), 1);
    assert_eq!(
        playback.output.try_recv().unwrap(),
        (21..=30).map(|n| n as f32).collect::<Vec<_>>()
    );
    playback.output.close();
    assert!(playback.poll().is_ready());
    assert_eq!(Arc::strong_count(&samples), 1);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn silence_is_clocked_and_batches_are_bounded() {
    for (requested, expected) in [(0, 1), (4096, 256)] {
        let mut playback = Playback::new(config(48_000, 2, requested), |_| {});
        assert!(playback.poll().is_pending());
        assert!(playback.output.is_empty());
        assert_eq!(playback.next_buffer(), vec![0.0; expected * 2]);
        drop(playback.task);
        assert!(playback.output.is_closed());
    }
}
