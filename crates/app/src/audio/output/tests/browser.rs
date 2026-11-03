use super::super::Output;
use wasm_bindgen::prelude::*;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
async fn queued_pcm_waits_for_play_and_drop_closes_submission() {
    let (output, config, samples) = Output::open(|error| console_error!("{error}")).unwrap();
    for _ in 0..samples.capacity().unwrap() {
        samples
            .try_send(vec![0.25; usize::from(config.channels)])
            .unwrap();
    }
    futures_timer::Delay::new(std::time::Duration::from_millis(50)).await;
    assert!(samples.is_full());
    drop(output);
    assert!(samples.is_closed());
    assert!(samples
        .try_send(vec![0.0; usize::from(config.channels)])
        .is_err());
}

#[wasm_bindgen_test]
fn drop_before_worklet_setup_closes_submission() {
    let (output, _, samples) = Output::open(|error| console_error!("{error}")).unwrap();
    drop(output);
    assert!(samples.is_closed());
}

#[wasm_bindgen(module = "/../../tests/browser/audio-output.js")]
extern "C" {
    #[wasm_bindgen(js_name = failNextModule)]
    fn fail_next_module();
}

#[wasm_bindgen_test]
async fn asynchronous_setup_errors_reach_the_application_callback() {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    fail_next_module();
    let reported = Arc::new(AtomicBool::new(false));
    let callback_reported = reported.clone();
    let (output, _, writer) = Output::open(move |error| {
        assert!(error.to_string().contains("test module failure"));
        callback_reported.store(true, Ordering::Release);
    })
    .unwrap();
    for _ in 0..300 {
        if reported.load(Ordering::Acquire) {
            break;
        }
        futures_timer::Delay::new(std::time::Duration::from_millis(10)).await;
    }
    assert!(reported.load(Ordering::Acquire));
    drop(output);
    assert!(writer.is_closed());
}
