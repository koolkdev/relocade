#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use relocade_audio::{Output, WriteError};
    use std::{sync::mpsc, thread, time::Duration};
    let (errors, reported) = mpsc::channel();
    let (output, mut writer) = Output::open(move |error| {
        let _ = errors.send(error);
    })?;
    let format = writer.format();
    let channels = usize::from(format.channels());
    let mut samples = vec![0.0; 128 * channels];
    let mut position = 0;
    output.play()?;
    while position < format.sample_rate() {
        if let Ok(error) = reported.try_recv() {
            return Err(error.into());
        }
        let frames = (format.sample_rate() - position).min(128) as usize;
        for (offset, frame) in samples[..frames * channels]
            .chunks_exact_mut(channels)
            .enumerate()
        {
            let phase = f64::from(position + offset as u32) * 440.0 * std::f64::consts::TAU
                / f64::from(format.sample_rate());
            frame.fill(phase.sin() as f32 * 0.2);
        }
        match writer.try_write(&samples[..frames * channels]) {
            Ok(()) => position += frames as u32,
            Err(WriteError::Full { .. }) => thread::sleep(Duration::from_millis(2)),
            Err(error) => return Err(error.into()),
        }
    }
    // Allow the bounded tail to reach the device before closing it.
    thread::sleep(Duration::from_secs_f64(
        writer.capacity_frames() as f64 / f64::from(format.sample_rate()) + 0.1,
    ));
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn main() {}
