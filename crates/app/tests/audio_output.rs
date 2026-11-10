use relocade_app::audio::Audio;
use std::{
    future::Future,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst},
        Arc,
    },
    time::Duration,
};
use web_thread::Thread;

#[path = "audio_output/execution.rs"]
mod execution;

struct ReleaseProducer(Arc<AtomicBool>);
impl Drop for ReleaseProducer {
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
    let (audio, config, samples) = Audio::open(|error| panic!("{error}")).unwrap();
    let submission = samples.clone();
    let initial_frames = (samples.capacity().unwrap() + 1) * 128;
    #[cfg(target_arch = "wasm32")]
    let buffered_frames = (initial_frames + 128) as u32;
    let _rate = config.sample_rate;
    let (frequencies, updates) = async_channel::bounded(1);
    let rendered = Arc::new(AtomicUsize::new(0));
    let worker_rendered = rendered.clone();
    let hold = Arc::new(AtomicBool::new(false));
    let _release = ReleaseProducer(hold.clone());
    let worker_hold = hold.clone();
    let holding = Arc::new(AtomicBool::new(false));
    let worker_holding = holding.clone();

    // The caller owns its producer. Audio only consumes its ordinary Rust channel.
    let producer = Thread::new();
    let task = producer.run_send((), move |()| async move {
        let channels = usize::from(config.channels);
        let mut frequency = 440;
        let mut position = 0u64;
        loop {
            if worker_hold.load(SeqCst) {
                worker_holding.store(true, SeqCst);
                while worker_hold.load(SeqCst) {
                    std::hint::spin_loop();
                }
            }
            if let Ok(update) = updates.try_recv() {
                frequency = update;
            }
            let mut buffer = vec![0.0; 128 * channels];
            for frame in buffer.chunks_exact_mut(channels) {
                let phase = position as f64 * f64::from(frequency) * std::f64::consts::TAU
                    / f64::from(config.sample_rate);
                frame.fill(phase.sin() as f32 * 0.2);
                position += 1;
            }
            worker_rendered.fetch_add(128, SeqCst);
            if samples.send(buffer).await.is_err() {
                return;
            }
        }
    });
    until(|| rendered.load(SeqCst) == initial_frames).await;
    #[cfg(target_arch = "wasm32")]
    until(|| observer.ready()).await;
    delay(50).await;
    assert_eq!(
        rendered.load(SeqCst),
        initial_frames,
        "unplayed output bounds production to the queue plus one batch"
    );
    audio.play().unwrap();
    until(|| rendered.load(SeqCst) > initial_frames).await;
    #[cfg(target_arch = "wasm32")]
    observer.check(_rate, 440, buffered_frames).await.unwrap();

    // The platform callback drains to silence while the producer is stalled.
    hold.store(true, SeqCst);
    until(|| holding.load(SeqCst)).await;
    let stalled = rendered.load(SeqCst);
    delay(200).await;
    assert_eq!(rendered.load(SeqCst), stalled);
    #[cfg(target_arch = "wasm32")]
    observer.check(_rate, 0, buffered_frames).await.unwrap();
    hold.store(false, SeqCst);
    until(|| rendered.load(SeqCst) > stalled).await;
    #[cfg(target_arch = "wasm32")]
    observer.check(_rate, 440, buffered_frames).await.unwrap();

    frequencies.send(660).await.unwrap();
    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen::JsCast;
        let memory: js_sys::WebAssembly::Memory = wasm_bindgen::memory().unchecked_into();
        memory.grow(2);
    }
    #[cfg(target_arch = "wasm32")]
    observer.check(_rate, 660, buffered_frames).await.unwrap();

    drop(audio);
    assert!(submission.is_closed());
    timeout(task).await.unwrap();
    assert!(frequencies.is_closed());
    #[cfg(target_arch = "wasm32")]
    observer.finish();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
#[ignore = "requires a default audio output; CI uses ALSA null"]
fn output_accepts_an_independently_owned_producer() {
    pollster::block_on(exercise_output());
}

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
#[ignore = "AudioWorklet bugs: https://github.com/koolkdev/relocade/issues/970 and https://github.com/koolkdev/relocade/issues/971"]
async fn output_accepts_an_independently_owned_producer() {
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
        pub async fn check(
            this: &Observer,
            rate: u32,
            frequency: u32,
            buffered_frames: u32,
        ) -> Result<(), JsValue>;
        #[wasm_bindgen(method)]
        pub fn finish(this: &Observer);
    }
}
