use super::super::Output;
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

#[path = "signal.rs"]
mod signal;
