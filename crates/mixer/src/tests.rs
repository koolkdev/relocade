use super::Mixer;
use relocade_clock_test_support::ManualClock;
use relocade_machine::Device;
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
    mixer: Rc<RefCell<Mixer>>,
    clock: ManualClock,
    output: async_channel::Receiver<Vec<f32>>,
    task: Pin<Box<dyn Future<Output = ()>>>,
}

fn config(rate: u32, channels: u16, batch: u32) -> cpal::StreamConfig {
    cpal::StreamConfig {
        sample_rate: rate,
        channels,
        buffer_size: cpal::BufferSize::Fixed(batch),
    }
}

impl Playback {
    fn new(config: cpal::StreamConfig) -> Self {
        let clock = ManualClock::new();
        let (samples, output) = async_channel::bounded(1);
        let mixer = Rc::new(RefCell::new(
            Mixer::new(clock.clock(), config, samples).unwrap(),
        ));
        Self {
            task: Box::pin(Mixer::run(mixer.clone())),
            mixer,
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
        self.clock.advance_to(self.clock.next_deadline().unwrap());
        assert!(self.poll().is_pending());
        self.output.try_recv().unwrap()
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn sources_sum_with_gains_on_the_same_sample_intervals() {
    let mut playback = Playback::new(config(1_000, 2, 4));
    let clock = playback.clock.clock();
    let start = clock.now();
    let intervals = Rc::new(RefCell::new(Vec::new()));
    let recorded = intervals.clone();
    playback.mixer.borrow_mut().add_source(0.5, move |interval| {
        recorded.borrow_mut().push(interval.clone());
        if clock.duration_since(interval.start, start).as_millis().is_multiple_of(2) {
            1.0
        } else {
            -1.0
        }
    });
    let other_intervals = Rc::new(RefCell::new(Vec::new()));
    let recorded = other_intervals.clone();
    playback.mixer.borrow_mut().add_source(2.0, move |interval| {
        recorded.borrow_mut().push(interval);
        0.5
    });
    playback.mixer.borrow_mut().add_source(1.0, |_| 0.0);
    assert_eq!(
        playback.next_buffer(),
        [1.5, 1.5, 0.5, 0.5, 1.5, 1.5, 0.5, 0.5]
    );
    assert_eq!(*intervals.borrow(), *other_intervals.borrow());
    for (index, interval) in intervals.borrow().iter().enumerate() {
        let clock = playback.clock.clock();
        assert_eq!(
            interval.start,
            clock.add(start, Duration::from_millis(index as u64))
        );
        assert_eq!(
            interval.end,
            clock.add(start, Duration::from_millis(index as u64 + 1))
        );
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn full_output_keeps_recent_samples_and_advances_sources() {
    let mut playback = Playback::new(config(1_000, 1, 10));
    let samples = Rc::new(RefCell::new(0));
    let rendered = samples.clone();
    playback.mixer.borrow_mut().add_source(1.0, move |_| {
        *rendered.borrow_mut() += 1;
        *rendered.borrow() as f32
    });
    assert!(playback.poll().is_pending());
    playback.clock.advance_by(Duration::from_millis(30));
    assert!(playback.poll().is_pending());
    assert_eq!(*samples.borrow(), 30);
    assert_eq!(playback.output.len(), 1);
    assert_eq!(
        playback.output.try_recv().unwrap(),
        (21..=30).map(|sample| sample as f32).collect::<Vec<_>>()
    );
    playback.output.close();
    assert!(playback.poll().is_ready());
    // The mixer retained on the bus must not retain a stopped source.
    assert_eq!(Rc::strong_count(&samples), 1);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn silence_is_clocked_and_batches_are_bounded() {
    let mut playback = Playback::new(config(48_000, 2, 4096));
    assert!(playback.poll().is_pending());
    assert!(playback.poll().is_pending());
    assert!(playback.output.is_empty());
    assert_eq!(playback.next_buffer(), vec![0.0; 256 * 2]);
    playback.output.close();
    assert!(playback.poll().is_ready());

    let playback = Playback::new(config(48_000, 1, 128));
    drop(playback.task);
    drop(playback.mixer);
    assert!(playback.output.is_closed());
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
        assert!(Mixer::new(ManualClock::new().clock(), config, samples).is_err());
    }
    let (samples, _output) = async_channel::unbounded();
    assert!(Mixer::new(ManualClock::new().clock(), config(48_000, 1, 128), samples).is_err());
}
