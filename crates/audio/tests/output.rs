#![cfg(not(target_arch = "wasm32"))]

use relocade_audio::Output;
use std::time::{Duration, Instant};

#[test]
#[ignore = "requires an audio output; CI uses ALSA null"]
fn output_starts_on_play_and_drop_closes_submission() {
    let (errors, reported) = std::sync::mpsc::channel();
    let (output, config, writer) = Output::open(move |error| {
        let _ = errors.send(error);
    })
    .unwrap();
    let channels = usize::from(config.channels);
    for _ in 0..writer.capacity().unwrap() {
        writer.try_send(vec![0.25; 128 * channels]).unwrap();
    }
    std::thread::sleep(Duration::from_millis(10));
    assert!(writer.is_full());
    output.play().unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while !writer.is_empty() {
        assert!(
            Instant::now() < deadline,
            "output did not consume PCM: {:?}",
            reported.try_recv()
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    drop(output);
    assert!(writer.is_closed());
    assert!(writer.try_send(vec![0.0; channels]).is_err());
}
