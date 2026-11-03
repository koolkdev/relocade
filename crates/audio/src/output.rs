use crate::{pcm_buffer, ConfigError, Format, PcmReader, PcmWriter};
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    FromSample, SampleFormat, SizedSample,
};
use std::{
    error::Error,
    fmt,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

/// Playback on the default device. Move the returned writer to the application's
/// producer. Dropping this owner stops playback and closes further submission.
pub struct Output {
    stream: cpal::Stream,
    playing: Arc<AtomicBool>,
    submission: async_channel::Receiver<f32>,
}

impl Output {
    /// Opens the default device with its preferred format and a 2048-frame queue.
    /// Read `writer.format()` before producing interleaved f32 samples, then call
    /// [`Self::play`]. CPAL reports stream errors through `on_error`; keep that
    /// callback brief because it may run on an audio thread.
    pub fn open(
        mut on_error: impl FnMut(OutputError) + Send + 'static,
    ) -> Result<(Self, PcmWriter), OutputError> {
        let device = host()?
            .default_output_device()
            .ok_or(OutputError::Unavailable)?;
        let config = device.default_output_config().map_err(backend_error)?;
        let format = Format::new(config.sample_rate(), config.channels())?;
        let (writer, reader) = pcm_buffer(format, 2048)?;
        let submission = reader.receiver.clone();
        let playing = Arc::new(AtomicBool::new(false));
        let report = move |error| on_error(backend_error(error));
        let stream = match config.sample_format() {
            SampleFormat::F32 => {
                build::<f32>(&device, config.into(), reader, playing.clone(), report)
            }
            SampleFormat::F64 => {
                build::<f64>(&device, config.into(), reader, playing.clone(), report)
            }
            SampleFormat::I8 => {
                build::<i8>(&device, config.into(), reader, playing.clone(), report)
            }
            SampleFormat::I16 => {
                build::<i16>(&device, config.into(), reader, playing.clone(), report)
            }
            SampleFormat::I24 => {
                build::<cpal::I24>(&device, config.into(), reader, playing.clone(), report)
            }
            SampleFormat::I32 => {
                build::<i32>(&device, config.into(), reader, playing.clone(), report)
            }
            SampleFormat::I64 => {
                build::<i64>(&device, config.into(), reader, playing.clone(), report)
            }
            SampleFormat::U8 => {
                build::<u8>(&device, config.into(), reader, playing.clone(), report)
            }
            SampleFormat::U16 => {
                build::<u16>(&device, config.into(), reader, playing.clone(), report)
            }
            SampleFormat::U24 => {
                build::<cpal::U24>(&device, config.into(), reader, playing.clone(), report)
            }
            SampleFormat::U32 => {
                build::<u32>(&device, config.into(), reader, playing.clone(), report)
            }
            SampleFormat::U64 => {
                build::<u64>(&device, config.into(), reader, playing.clone(), report)
            }
            _ => {
                return Err(OutputError::Backend(
                    "unsupported device sample representation".into(),
                ))
            }
        }
        .map_err(backend_error)?;
        Ok((
            Self {
                stream,
                playing,
                submission,
            },
            writer,
        ))
    }

    /// Starts consuming queued samples. Missing samples become silence.
    /// In browsers this requests playback; it does not await worklet readiness
    /// or the AudioContext resume promise. Call from a user gesture.
    pub fn play(&self) -> Result<(), OutputError> {
        self.stream.play().map_err(backend_error)?;
        self.playing.store(true, Ordering::Release);
        Ok(())
    }
}

impl Drop for Output {
    fn drop(&mut self) {
        self.playing.store(false, Ordering::Release);
        // Browser teardown is asynchronous. Close submission immediately while
        // CPAL retains ownership of the callback and its reader.
        self.submission.close();
    }
}

fn host() -> Result<cpal::Host, OutputError> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        Ok(cpal::default_host())
    }
    #[cfg(all(target_arch = "wasm32", target_feature = "atomics"))]
    {
        cpal::host_from_id(cpal::HostId::AudioWorklet).map_err(backend_error)
    }
    #[cfg(all(target_arch = "wasm32", not(target_feature = "atomics")))]
    {
        Err(OutputError::Backend(
            "browser audio requires atomics-enabled Wasm".into(),
        ))
    }
}

fn build<T: SizedSample + FromSample<f32>>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    mut reader: PcmReader,
    playing: Arc<AtomicBool>,
    on_error: impl FnMut(cpal::Error) + Send + 'static,
) -> Result<cpal::Stream, cpal::Error> {
    device.build_output_stream(
        config,
        move |samples: &mut [T], _| {
            if playing.load(Ordering::Acquire) {
                // Convert directly into the host buffer, without allocating in
                // the callback. CPAL supplies complete interleaved frames.
                reader
                    .read_mapped(samples, T::from_sample)
                    .expect("device supplied incomplete frames");
            } else {
                samples.fill(T::EQUILIBRIUM);
            }
        },
        on_error,
        None,
    )
}

fn backend_error(error: cpal::Error) -> OutputError {
    OutputError::Backend(error.to_string())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OutputError {
    Config(ConfigError),
    Unavailable,
    Backend(String),
}

impl fmt::Display for OutputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(error) => error.fmt(f),
            Self::Unavailable => f.write_str("no audio output is available"),
            Self::Backend(message) => f.write_str(message),
        }
    }
}
impl Error for OutputError {}
impl From<ConfigError> for OutputError {
    fn from(error: ConfigError) -> Self {
        Self::Config(error)
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use cpal::Sample;

    #[test]
    fn host_conversion_preserves_signed_samples_and_unsigned_silence() {
        let format = Format::new(48_000, 1).unwrap();
        let (mut writer, mut reader) = pcm_buffer(format, 4).unwrap();
        writer.try_write(&[-1.0, 0.0, 0.5]).unwrap();
        let mut signed = [0i16; 4];
        reader.read_mapped(&mut signed, i16::from_sample).unwrap();
        assert_eq!(signed, [i16::MIN, 0, 16384, 0]);
        writer.try_write(&[-1.0, 0.0, 0.5]).unwrap();
        let mut unsigned = [0u16; 4];
        reader.read_mapped(&mut unsigned, u16::from_sample).unwrap();
        assert_eq!(unsigned, [0, 32768, 49152, 32768]);
    }
}
