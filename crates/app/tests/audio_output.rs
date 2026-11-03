use async_channel::{Receiver, Sender, TryRecvError};
use relocade_app::{
    audio::{cpal, Audio},
    worker::Thread,
};
use std::{
    future::Future,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst},
        Arc,
    },
    time::Duration,
};

struct Tone {
    config: cpal::StreamConfig,
    samples: Sender<Vec<f32>>,
    sources: Receiver<Receiver<u32>>,
    source: Option<Receiver<u32>>,
    frequency: u32,
    position: u64,
    rendered: Arc<AtomicUsize>,
    hold: Arc<AtomicBool>,
    holding: Arc<AtomicBool>,
}

impl Tone {
    async fn run(mut self) -> Result<(), String> {
        let config = self.config;
        let channels = usize::from(config.channels);
        loop {
            if self.hold.load(SeqCst) {
                self.holding.store(true, SeqCst);
                while self.hold.load(SeqCst) {
                    std::hint::spin_loop();
                }
            }
            loop {
                if let Some(source) = &self.source {
                    match source.try_recv() {
                        Ok(frequency) => self.frequency = frequency,
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Closed) => self.source = None,
                    }
                } else {
                    // Ending one producer leaves the device waiting for another
                    // ordinary Rust receiver, while output drains into silence.
                    let source = self.sources.recv().await.map_err(|e| e.to_string())?;
                    if let Ok(frequency) = source.recv().await {
                        self.frequency = frequency;
                        self.source = Some(source);
                    }
                }
            }
            let mut samples = vec![0.0; 128 * channels];
            for frame in samples.chunks_exact_mut(channels) {
                let phase =
                    self.position as f64 * f64::from(self.frequency) * std::f64::consts::TAU
                        / f64::from(config.sample_rate);
                frame.fill(phase.sin() as f32 * 0.2);
                self.position += 1;
            }
            self.rendered.fetch_add(samples.len() / channels, SeqCst);
            if self.samples.send(samples).await.is_err() {
                return Ok(());
            }
            // An unpaced test sink can keep sends ready; cooperate with shutdown.
            futures_lite::future::yield_now().await;
        }
    }
}

struct StopExecution(Arc<AtomicBool>);
impl Drop for StopExecution {
    fn drop(&mut self) {
        self.0.store(true, SeqCst);
    }
}

struct ReleaseDevice(Arc<AtomicBool>);
impl Drop for ReleaseDevice {
    fn drop(&mut self) {
        self.0.store(false, SeqCst);
    }
}

async fn delay(milliseconds: u64) {
    futures_timer::Delay::new(Duration::from_millis(milliseconds)).await;
}

async fn until(mut ready: impl FnMut() -> bool) {
    for _ in 0..500 {
        if ready() {
            return;
        }
        delay(10).await;
    }
    panic!("audio output fixture timed out");
}

async fn timeout<T>(future: impl Future<Output = T>) -> T {
    futures_lite::future::or(future, async {
        delay(5000).await;
        panic!("audio output completion timed out");
    })
    .await
}

async fn exercise_output() {
    #[cfg(target_arch = "wasm32")]
    let observer = browser::observe_output();
    let (sources, incoming) = async_channel::bounded(1);
    let (frequency, source) = async_channel::bounded(1);
    frequency.try_send(440).unwrap();
    sources.try_send(source).unwrap();
    let rendered = Arc::new(AtomicUsize::new(0));
    let worker_rendered = rendered.clone();
    let hold = Arc::new(AtomicBool::new(false));
    let _release = ReleaseDevice(hold.clone());
    let worker_hold = hold.clone();
    let holding = Arc::new(AtomicBool::new(false));
    let worker_holding = holding.clone();
    let (configured, configuration) = futures_channel::oneshot::channel();
    let mut audio = Audio::attach_device(
        move |config, samples| async move {
            configured
                .send((config.sample_rate, samples.capacity().unwrap()))
                .unwrap();
            Tone {
                config,
                samples,
                sources: incoming,
                source: None,
                frequency: 0,
                position: 0,
                rendered: worker_rendered,
                hold: worker_hold,
                holding: worker_holding,
            }
            .run()
            .await
        },
        |error| panic!("{error}"),
    )
    .unwrap();
    let (_rate, capacity) = timeout(configuration).await.unwrap();
    let initial_frames = (capacity + 1) * 128;
    let stop = Arc::new(AtomicBool::new(false));
    let _stop = StopExecution(stop.clone());
    let worker_stop = stop.clone();
    let heartbeat = Arc::new(AtomicUsize::new(0));
    let worker_heartbeat = heartbeat.clone();
    let execution = Thread::new();
    let task = execution.run_send((), move |()| async move {
        let _source = frequency;
        // Execution cannot dispatch browser tasks or Rust futures while busy.
        // Audio must still render and feed its separate platform output.
        while !worker_stop.load(SeqCst) {
            worker_heartbeat.fetch_add(1, SeqCst);
            std::hint::spin_loop();
        }
    });
    until(|| rendered.load(SeqCst) == initial_frames && heartbeat.load(SeqCst) > 0).await;
    #[cfg(target_arch = "wasm32")]
    until(|| observer.ready()).await;
    delay(50).await;
    assert_eq!(
        rendered.load(SeqCst),
        initial_frames,
        "unplayed output bounds rendering to the queue plus one batch"
    );
    let busy = heartbeat.load(SeqCst);
    audio.play().unwrap();
    until(|| rendered.load(SeqCst) > initial_frames).await;
    delay(200).await;
    assert!(heartbeat.load(SeqCst) > busy);
    #[cfg(target_arch = "wasm32")]
    observer.check(_rate, 440).unwrap();

    // Output continues to silence even when its producer cannot dispatch tasks.
    hold.store(true, SeqCst);
    until(|| holding.load(SeqCst)).await;
    let stalled = rendered.load(SeqCst);
    delay(200).await;
    assert_eq!(rendered.load(SeqCst), stalled);
    #[cfg(target_arch = "wasm32")]
    observer.check(_rate, 0).unwrap();
    hold.store(false, SeqCst);
    until(|| rendered.load(SeqCst) > stalled).await;
    delay(200).await;
    #[cfg(target_arch = "wasm32")]
    observer.check(_rate, 440).unwrap();

    stop.store(true, SeqCst);
    timeout(task).await.unwrap(); // Execution drops its source, not the audio worker.
    delay(200).await;
    let idle = rendered.load(SeqCst);
    delay(30).await;
    assert_eq!(rendered.load(SeqCst), idle);
    assert!(futures_lite::future::poll_once(audio.join())
        .await
        .is_none());
    #[cfg(target_arch = "wasm32")]
    observer.check(_rate, 0).unwrap();

    let (replacement, source) = async_channel::bounded(1);
    replacement.try_send(660).unwrap();
    sources.send(source).await.unwrap();
    until(|| rendered.load(SeqCst) > idle).await;
    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen::JsCast;
        let memory: js_sys::WebAssembly::Memory = wasm_bindgen::memory().unchecked_into();
        memory.grow(2);
    }
    delay(200).await;
    #[cfg(target_arch = "wasm32")]
    observer.check(_rate, 660).unwrap();
    if let Some(result) = futures_lite::future::poll_once(audio.close()).await {
        result.unwrap();
    }
    timeout(audio.close()).await.unwrap();
    assert!(replacement.is_closed());
    assert!(audio.play().is_err());
    #[cfg(target_arch = "wasm32")]
    observer.finish();
}

async fn exercise_failed_device_close() {
    let (attached, attachment) = futures_channel::oneshot::channel();
    let mut audio = Audio::attach_device(
        move |_, samples| async move {
            attached.send(samples).unwrap();
            Err("device failed".into())
        },
        |error| panic!("{error}"),
    )
    .unwrap();
    let samples = timeout(attachment).await.unwrap();
    assert_eq!(timeout(audio.join()).await.unwrap_err(), "device failed");
    assert!(!samples.is_closed(), "join retains platform output");
    assert_eq!(timeout(audio.close()).await.unwrap_err(), "device failed");
    assert!(
        samples.is_closed(),
        "close releases output despite the error"
    );
    assert!(audio.play().is_err());
    assert_eq!(audio.close().await.unwrap_err(), "device failed");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
#[ignore = "requires a default audio output; CI uses ALSA null"]
fn close_releases_output_after_device_failure() {
    pollster::block_on(exercise_failed_device_close());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn close_releases_output_after_device_failure() {
    exercise_failed_device_close().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
#[ignore = "requires a default audio output; CI uses ALSA null"]
fn audio_worker_plays_independently_of_execution_and_replaces_sources() {
    pollster::block_on(exercise_output());
}

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn audio_worker_plays_independently_of_execution_and_replaces_sources() {
    exercise_output().await;
}

#[cfg(target_arch = "wasm32")]
mod browser {
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen(module = "/../../tests/browser/audio-output.js")]
    extern "C" {
        pub type Observer;
        #[wasm_bindgen(js_name = observeOutput)]
        pub fn observe_output() -> Observer;
        #[wasm_bindgen(method, getter)]
        pub fn ready(this: &Observer) -> bool;
        #[wasm_bindgen(method, catch)]
        pub fn check(this: &Observer, rate: u32, frequency: u32) -> Result<(), JsValue>;
        #[wasm_bindgen(method)]
        pub fn finish(this: &Observer);
    }
}
