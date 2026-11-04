use super::super::*;

fn buffers(channels: usize) -> (Sender<Vec<f32>>, Buffers) {
    let (sender, receiver) = async_channel::bounded(4);
    (
        sender,
        Buffers {
            receiver,
            pending: Vec::new().into_iter(),
            channels,
        },
    )
}

#[test]
fn buffers_span_callbacks_and_drain_after_sender_closure() {
    let (sender, mut reader) = buffers(2);
    sender.try_send(vec![0.25, -0.25, 0.5, -0.5]).unwrap();
    sender.try_send(vec![0.75, -0.75]).unwrap();
    drop(sender);
    let mut first = [0.0; 2];
    reader.fill(&mut first);
    assert_eq!(first, [0.25, -0.25]);
    let mut rest = [9.0; 6];
    reader.fill(&mut rest);
    assert_eq!(rest, [0.5, -0.5, 0.75, -0.75, 0.0, 0.0]);
}

#[test]
fn short_buffers_cannot_shift_channels_in_later_buffers() {
    let (sender, mut reader) = buffers(2);
    sender.try_send(vec![]).unwrap();
    sender.try_send(vec![0.9]).unwrap();
    sender.try_send(vec![0.5, -0.5, 0.9]).unwrap();
    sender.try_send(vec![0.25, -0.25]).unwrap();
    let mut samples = [9.0; 8];
    reader.fill(&mut samples);
    assert_eq!(samples, [0.0, 0.0, 0.0, 0.0, 0.5, -0.5, 0.25, -0.25]);
}

#[test]
fn normalization_and_conversion_preserve_host_silence() {
    let (sender, mut reader) = buffers(1);
    sender.try_send(vec![-2.0, f32::NAN, 2.0]).unwrap();
    let mut signed = [1i16; 4];
    reader.fill(&mut signed);
    assert_eq!(signed, [i16::MIN, 0, i16::MAX, 0]);
    sender.try_send(vec![-1.0, 0.0, 0.5]).unwrap();
    let mut unsigned = [1u16; 4];
    reader.fill(&mut unsigned);
    assert_eq!(unsigned, [0, 32768, 49152, 32768]);
}

use std::time::{Duration, Instant};

#[test]
#[ignore = "requires an audio output; CI uses ALSA null"]
fn output_starts_on_play_and_drop_closes_submission() {
    let (errors, reported) = std::sync::mpsc::channel();
    let (output, config, writer) = Audio::open(move |error| {
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
