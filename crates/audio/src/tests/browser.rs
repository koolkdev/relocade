use crate::{Output, WriteError};
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
async fn queued_pcm_waits_for_play_and_drop_closes_submission() {
    let (output, mut writer) = Output::open(|error| console_error!("{error}")).unwrap();
    let channels = usize::from(writer.format().channels());
    writer
        .try_write(&vec![0.25; channels * writer.capacity_frames()])
        .unwrap();
    futures_timer::Delay::new(std::time::Duration::from_millis(50)).await;
    assert_eq!(writer.available_frames(), 0);
    drop(output);
    assert!(writer.is_closed());
    assert_eq!(
        writer.try_write(&vec![0.0; channels]),
        Err(WriteError::Closed)
    );
}

#[wasm_bindgen_test]
fn drop_before_worklet_setup_closes_submission() {
    let (output, writer) = Output::open(|error| console_error!("{error}")).unwrap();
    drop(output);
    assert!(writer.is_closed());
}

#[path = "signal.rs"]
mod signal;
