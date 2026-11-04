//! CPAL output fed by ordinary bounded Rust sample buffers.

pub use cpal;

use async_channel::{Receiver, Sender};
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    FromSample, SampleFormat, SizedSample,
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

/// Playback on the default device. Create and play this on main.
/// Dropping it closes sample submission; the producer owns its own lifetime.
pub struct Audio {
    stream: cpal::Stream,
    playing: Arc<AtomicBool>,
    submission: Sender<Vec<f32>>,
}

impl Audio {
    /// Opens the default device and returns its CPAL configuration and a bounded
    /// Rust sender. Each buffer contains interleaved f32 frames. The channel holds
    /// 16 buffers; the producer chooses and bounds each buffer's size.
    pub fn open(
        on_error: impl FnMut(cpal::Error) + Send + 'static,
    ) -> Result<(Self, cpal::StreamConfig, Sender<Vec<f32>>), cpal::Error> {
        let device = host()?
            .default_output_device()
            .ok_or_else(|| cpal::Error::new(cpal::ErrorKind::DeviceNotAvailable))?;
        let config = device.default_output_config()?;
        let stream_config = config.config();
        let (submission, receiver) = async_channel::bounded(16);
        let reader = Buffers {
            receiver,
            pending: Vec::new().into_iter(),
            channels: usize::from(config.channels()),
        };
        let playing = Arc::new(AtomicBool::new(false));
        let stream = match config.sample_format() {
            SampleFormat::F32 => {
                build::<f32>(&device, config.into(), reader, playing.clone(), on_error)
            }
            SampleFormat::F64 => {
                build::<f64>(&device, config.into(), reader, playing.clone(), on_error)
            }
            SampleFormat::I8 => {
                build::<i8>(&device, config.into(), reader, playing.clone(), on_error)
            }
            SampleFormat::I16 => {
                build::<i16>(&device, config.into(), reader, playing.clone(), on_error)
            }
            SampleFormat::I24 => {
                build::<cpal::I24>(&device, config.into(), reader, playing.clone(), on_error)
            }
            SampleFormat::I32 => {
                build::<i32>(&device, config.into(), reader, playing.clone(), on_error)
            }
            SampleFormat::I64 => {
                build::<i64>(&device, config.into(), reader, playing.clone(), on_error)
            }
            SampleFormat::U8 => {
                build::<u8>(&device, config.into(), reader, playing.clone(), on_error)
            }
            SampleFormat::U16 => {
                build::<u16>(&device, config.into(), reader, playing.clone(), on_error)
            }
            SampleFormat::U24 => {
                build::<cpal::U24>(&device, config.into(), reader, playing.clone(), on_error)
            }
            SampleFormat::U32 => {
                build::<u32>(&device, config.into(), reader, playing.clone(), on_error)
            }
            SampleFormat::U64 => {
                build::<u64>(&device, config.into(), reader, playing.clone(), on_error)
            }
            _ => return Err(cpal::Error::new(cpal::ErrorKind::UnsupportedConfig)),
        }?;
        Ok((
            Self {
                stream,
                playing,
                submission: submission.clone(),
            },
            stream_config,
            submission,
        ))
    }

    /// Starts consuming PCM. In browsers this requests resume without awaiting
    /// the browser promise; call it from the application's user activation.
    pub fn play(&self) -> Result<(), cpal::Error> {
        self.stream.play()?;
        self.playing.store(true, Ordering::Release);
        Ok(())
    }
}

impl Drop for Audio {
    fn drop(&mut self) {
        self.playing.store(false, Ordering::Release);
        // CPAL owns asynchronous browser teardown; close submission immediately.
        self.submission.close();
    }
}

fn host() -> Result<cpal::Host, cpal::Error> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        Ok(cpal::default_host())
    }
    #[cfg(all(target_arch = "wasm32", target_feature = "atomics"))]
    {
        cpal::host_from_id(cpal::HostId::AudioWorklet)
    }
    #[cfg(all(target_arch = "wasm32", not(target_feature = "atomics")))]
    {
        Err(cpal::Error::with_message(
            cpal::ErrorKind::UnsupportedOperation,
            "browser audio requires atomics-enabled Wasm",
        ))
    }
}

fn build<T: SizedSample + FromSample<f32>>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    mut reader: Buffers,
    playing: Arc<AtomicBool>,
    on_error: impl FnMut(cpal::Error) + Send + 'static,
) -> Result<cpal::Stream, cpal::Error> {
    device.build_output_stream(
        config,
        move |samples: &mut [T], _| {
            if playing.load(Ordering::Acquire) {
                reader.fill(samples);
            } else {
                samples.fill(T::EQUILIBRIUM);
            }
        },
        on_error,
        None,
    )
}

struct Buffers {
    receiver: Receiver<Vec<f32>>,
    pending: std::vec::IntoIter<f32>,
    channels: usize,
}

impl Buffers {
    fn fill<T: SizedSample + FromSample<f32>>(&mut self, output: &mut [T]) {
        for frame in output.chunks_exact_mut(self.channels) {
            if self.pending.len() < self.channels {
                let mut buffer = self.receiver.try_recv().unwrap_or_default();
                // Do not let a partial trailing frame shift the next buffer's channels.
                buffer.truncate(buffer.len() / self.channels * self.channels);
                self.pending = buffer.into_iter();
            }
            // At most one receive per output frame, even with empty input buffers.
            for sample in frame {
                let value = self.pending.next().unwrap_or(0.0);
                *sample = T::from_sample(if value.is_nan() {
                    0.0
                } else {
                    value.clamp(-1.0, 1.0)
                });
            }
        }
    }
}

#[cfg(test)]
mod tests;
