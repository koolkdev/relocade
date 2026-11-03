#![cfg(not(target_arch = "wasm32"))]

use relocade_audio::{Output, WriteError};
use std::time::{Duration, Instant};

#[test]
#[ignore = "requires an audio output; CI uses ALSA null"]
fn output_starts_on_play_and_drop_closes_submission() {
    let (errors, reported) = std::sync::mpsc::channel();
    let (output, mut writer) = Output::open(move |error| {
        let _ = errors.send(error);
    })
    .unwrap();
    let channels = usize::from(writer.format().channels());
    writer
        .try_write(&vec![0.25; writer.capacity_frames() * channels])
        .unwrap();
    std::thread::sleep(Duration::from_millis(10));
    assert_eq!(writer.available_frames(), 0);
    output.play().unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while writer.available_frames() != writer.capacity_frames() {
        assert!(
            Instant::now() < deadline,
            "output did not consume PCM: {:?}",
            reported.try_recv()
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    drop(output);
    assert!(writer.is_closed());
    assert_eq!(
        writer.try_write(&vec![0.0; channels]),
        Err(WriteError::Closed)
    );
}
