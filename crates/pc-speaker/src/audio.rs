use crate::Speaker;
use async_channel::Sender;
use futures_lite::future::or;
use relocade_machine::Clock;
use std::{cell::RefCell, rc::Rc, time::Duration};

mod synthesis;
pub use synthesis::AudioSource;

impl Speaker {
    /// Attaches output once. The PIT's clock drives synthesis in either host
    /// or guest time; the PCM receiver supplies playback at the configured rate.
    /// Control changes are retained until their sample interval is rendered.
    pub fn attach_audio(
        &mut self,
        config: cpal::StreamConfig,
        samples: Sender<Vec<f32>>,
    ) -> Result<(), String> {
        if self.audio.is_some() || self.audio_ready.0.is_closed() {
            return Err("speaker output is already attached".into());
        }
        if config.sample_rate == 0 || config.sample_rate > 1_000_000_000 || config.channels == 0 {
            return Err(
                "speaker output needs a representable sample rate and nonzero channel count".into(),
            );
        }
        if samples.capacity().is_none() {
            return Err("speaker output needs a bounded sample channel".into());
        }
        let clock = self.pit.borrow().clock().clone();
        self.audio = Some(Audio {
            clock,
            config,
            samples,
            source: self.audio_source(),
        });
        self.audio_ready.0.try_send(()).unwrap();
        Ok(())
    }
}

pub(super) struct Audio {
    clock: Clock,
    config: cpal::StreamConfig,
    samples: Sender<Vec<f32>>,
    source: AudioSource,
}

pub(super) async fn run(device: Rc<RefCell<Speaker>>) {
    let ready = device.borrow().audio_ready.1.clone();
    ready.recv().await.unwrap();
    // Attachment remains one-shot after the activity takes ownership.
    ready.close();
    let Audio {
        clock,
        config,
        samples,
        mut source,
    } = device.borrow_mut().audio.take().unwrap();
    let started_at = source.started_at();
    let rate = u64::from(config.sample_rate);
    let frame_time = |frame: u64| {
        clock.add(
            started_at,
            Duration::new(
                frame / rate,
                ((frame % rate) * 1_000_000_000).div_ceil(rate) as u32,
            ),
        )
    };
    let batch = match config.buffer_size {
        cpal::BufferSize::Fixed(frames) => frames.clamp(1, 256) as usize,
        cpal::BufferSize::Default => 256,
    };
    let mut frames = 0;
    while !samples.is_closed() {
        or(
            clock.sleep_until(frame_time(frames + batch as u64)),
            samples.closed(),
        )
        .await;
        if samples.is_closed() {
            return;
        }
        let mut buffer = vec![0.0; batch * usize::from(config.channels)];
        for frame in buffer.chunks_exact_mut(usize::from(config.channels)) {
            frame.fill(0.2 * source.sample(frame_time(frames)..frame_time(frames + 1)));
            frames += 1;
        }
        // An unplayed or stalled sink keeps recent PCM. Do not leave register
        // changes accumulating while the output channel is full.
        if samples.force_send(buffer).is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests;
