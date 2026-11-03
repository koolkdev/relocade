use crate::Output;
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc,
    },
    time::Duration,
};
use wasm_bindgen::prelude::*;
use wasm_bindgen_test::*;

#[wasm_bindgen(module = "/tests/web-signal.js")]
extern "C" {
    type Observer;
    #[wasm_bindgen(js_name = observeOutput)]
    fn observe_output() -> Observer;
    #[wasm_bindgen(method, getter)]
    fn ready(this: &Observer) -> bool;
    #[wasm_bindgen(method, catch)]
    fn check(this: &Observer, rate: u32, frequency: u32) -> Result<(), JsValue>;
    #[wasm_bindgen(method)]
    fn finish(this: &Observer);
    #[wasm_bindgen(js_name = failNextModule)]
    fn fail_next_module();
}

struct StopProducer(Arc<AtomicBool>);
impl Drop for StopProducer {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

async fn delay(milliseconds: u64) {
    futures_timer::Delay::new(Duration::from_millis(milliseconds)).await;
}

async fn until(mut condition: impl FnMut() -> bool) {
    let deadline = js_sys::Date::now() + 3000.0;
    while !condition() {
        assert!(js_sys::Date::now() < deadline, "audio made no progress");
        delay(10).await;
    }
}

#[wasm_bindgen_test]
async fn rust_worker_pcm_reaches_output_across_underrun_and_memory_growth() {
    let observer = observe_output();
    let (output, mut writer) = Output::open(|error| console_error!("{error}")).unwrap();
    let format = writer.format();
    let rate = format.sample_rate();
    let channels = usize::from(format.channels());
    let capacity = writer.capacity_frames() as u32;
    let stop = Arc::new(AtomicBool::new(false));
    // Zero holds production so the output drains into underrun silence.
    let frequency = Arc::new(AtomicU32::new(440));
    let submitted = Arc::new(AtomicU32::new(0));
    let heartbeat = Arc::new(AtomicU32::new(0));
    let worker_stop = stop.clone();
    let worker_frequency = frequency.clone();
    let worker_submitted = submitted.clone();
    let worker_heartbeat = heartbeat.clone();
    let thread = web_thread::Thread::new();
    let producer = thread.run_send((), move |()| async move {
        let mut samples = vec![0.0; 128 * channels];
        let mut position = 0u64;
        // Deliberately keep this fixture worker busy while Web Audio consumes
        // its shared Rust queue. Production scheduling belongs to the application.
        while !worker_stop.load(Ordering::Acquire) {
            worker_heartbeat.fetch_add(1, Ordering::Relaxed);
            let hz = worker_frequency.load(Ordering::Acquire);
            if hz == 0 || writer.available_frames() < 128 {
                std::hint::spin_loop();
                continue;
            }
            for (offset, frame) in samples.chunks_exact_mut(channels).enumerate() {
                let phase =
                    (position + offset as u64) as f64 * f64::from(hz) * std::f64::consts::TAU
                        / f64::from(rate);
                frame.fill(phase.sin() as f32 * 0.2);
            }
            if writer.try_write(&samples).is_err() {
                break;
            }
            position += 128;
            worker_submitted.fetch_add(128, Ordering::Release);
        }
        writer
    });
    let playback = async {
        let _stop = StopProducer(stop.clone());
        until(|| submitted.load(Ordering::Acquire) == capacity && observer.ready()).await;
        let busy = heartbeat.load(Ordering::Acquire);
        delay(50).await;
        assert_eq!(submitted.load(Ordering::Acquire), capacity);
        assert!(heartbeat.load(Ordering::Acquire) > busy);
        output.play().unwrap();
        until(|| submitted.load(Ordering::Acquire) > capacity).await;
        delay(200).await;
        observer.check(rate, 440).unwrap();
        frequency.store(0, Ordering::Release);
        delay(200).await;
        observer.check(rate, 0).unwrap();
        frequency.store(660, Ordering::Release);
        let memory: js_sys::WebAssembly::Memory = wasm_bindgen::memory().unchecked_into();
        memory.grow(2);
        delay(200).await;
        observer.check(rate, 660).unwrap();
        drop(output);
        observer.finish();
    };
    let (writer, ()) = futures_util::future::join(producer, playback).await;
    assert!(writer.unwrap().is_closed());
}

#[wasm_bindgen_test]
async fn asynchronous_setup_errors_reach_the_application_callback() {
    fail_next_module();
    let reported = Arc::new(AtomicBool::new(false));
    let callback_reported = reported.clone();
    let (output, writer) = Output::open(move |error| {
        assert!(error.to_string().contains("test module failure"));
        callback_reported.store(true, Ordering::Release);
    })
    .unwrap();
    until(|| reported.load(Ordering::Acquire)).await;
    drop(output);
    assert!(writer.is_closed());
}
