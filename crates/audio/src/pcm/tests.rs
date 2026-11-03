use super::*;

#[test]
fn stereo_order_backpressure_and_underrun_silence() {
    let (mut writer, mut reader) = pcm_buffer(Format::new(44_100, 2).unwrap(), 3).unwrap();
    assert_eq!(writer.capacity_frames(), 3);
    writer.try_write(&[0.1, -0.1, 0.2, -0.2]).unwrap();
    assert_eq!(
        writer.try_write(&[0.3, -0.3, 0.4, -0.4]),
        Err(WriteError::Full {
            available_frames: 1
        })
    );
    assert_eq!(writer.try_write(&[0.4]), Err(WriteError::IncompleteFrame));
    let mut first = [9.0; 2];
    assert_eq!(reader.read(&mut first).unwrap(), 1);
    assert_eq!(first, [0.1, -0.1]);
    writer.try_write(&[0.3, -0.3, 0.4, -0.4]).unwrap();
    let mut tail = [9.0; 8];
    assert_eq!(reader.read(&mut tail).unwrap(), 3);
    assert_eq!(tail, [0.2, -0.2, 0.3, -0.3, 0.4, -0.4, 0.0, 0.0]);
}

#[test]
fn writer_drop_preserves_the_tail_and_reader_drop_closes_submission() {
    let (mut writer, mut reader) = pcm_buffer(Format::new(48_000, 1).unwrap(), 4).unwrap();
    writer.try_write(&[0.25, 0.5]).unwrap();
    drop(writer);
    let mut output = [9.0; 3];
    assert_eq!(reader.read(&mut output).unwrap(), 2);
    assert_eq!(output, [0.25, 0.5, 0.0]);
    let (mut writer, reader) = pcm_buffer(Format::new(48_000, 1).unwrap(), 4).unwrap();
    let _output_control = reader.receiver.clone();
    drop(reader);
    assert!(writer.is_closed());
    assert_eq!(writer.try_write(&[0.0]), Err(WriteError::Closed));
}

#[test]
fn normalization_and_invalid_shapes_do_not_consume_frames() {
    assert!(Format::new(0, 2).is_err());
    assert!(Format::new(48_000, 0).is_err());
    assert!(Format::new(48_000, 33).is_err());
    let format = Format::new(48_000, 2).unwrap();
    assert!(pcm_buffer(format, 0).is_err());
    assert!(pcm_buffer(format, 65_537).is_err());
    let (mut writer, mut reader) = pcm_buffer(format, 2).unwrap();
    writer
        .try_write(&[f32::NAN, f32::INFINITY, -2.0, 0.5])
        .unwrap();
    assert!(reader.read(&mut [0.0; 1]).is_err());
    let mut output = [0.0; 4];
    reader.read(&mut output).unwrap();
    assert_eq!(output, [0.0, 1.0, -1.0, 0.5]);
}

#[test]
fn concurrent_producer_preserves_complete_frames() {
    let (mut writer, mut reader) = pcm_buffer(Format::new(48_000, 2).unwrap(), 8).unwrap();
    let producer = std::thread::spawn(move || {
        for n in 1..=10_000 {
            let value = n as f32 / 10_000.0;
            loop {
                match writer.try_write(&[value, -value]) {
                    Ok(()) => break,
                    Err(WriteError::Full { .. }) => std::thread::yield_now(),
                    result => panic!("unexpected write: {result:?}"),
                }
            }
        }
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut received = 0;
    while received < 10_000 {
        assert!(std::time::Instant::now() < deadline, "PCM transfer stalled");
        let mut output = [0.0; 6];
        let frames = reader.read(&mut output).unwrap();
        for frame in output[..frames * 2].as_chunks::<2>().0 {
            received += 1;
            let expected = received as f32 / 10_000.0;
            assert_eq!(*frame, [expected, -expected]);
        }
        std::thread::yield_now();
    }
    producer.join().unwrap();
}
