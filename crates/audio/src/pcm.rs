use crate::{ConfigError, Format};
use async_channel::{Receiver, Sender, TrySendError};
use std::{error::Error, fmt};

/// Connects one producer to one output through a bounded Rust channel.
///
/// Capacity is between 1 and 65,536 frames. Neither endpoint is cloneable;
/// move the writer to the application's audio worker. Submission and reads
/// use `try_send` and `try_recv` without awaiting channel readiness.
pub fn pcm_buffer(
    format: Format,
    capacity_frames: usize,
) -> Result<(PcmWriter, PcmReader), ConfigError> {
    if !(1..=65_536).contains(&capacity_frames) {
        return Err(ConfigError(
            "buffer capacity must be between 1 and 65536 frames",
        ));
    }
    let (sender, receiver) =
        async_channel::bounded(capacity_frames * usize::from(format.channels()));
    Ok((PcmWriter { sender, format }, PcmReader { receiver, format }))
}

/// Unique producer of complete PCM frames. Dropping it lets queued samples drain.
pub struct PcmWriter {
    sender: Sender<f32>,
    format: Format,
}

impl PcmWriter {
    pub fn format(&self) -> Format {
        self.format
    }

    pub fn capacity_frames(&self) -> usize {
        self.sender.capacity().unwrap() / usize::from(self.format.channels())
    }

    /// Conservative free space while the output consumes samples concurrently.
    pub fn available_frames(&self) -> usize {
        (self.sender.capacity().unwrap() - self.sender.len()) / usize::from(self.format.channels())
    }

    pub fn is_closed(&self) -> bool {
        self.sender.is_closed()
    }

    /// Accepts a whole batch or reports insufficient space. NaN becomes silence;
    /// other samples are clamped to [-1, 1]. A concurrent output closure can
    /// interrupt a batch. Batches larger than capacity must be split by the caller.
    pub fn try_write(&mut self, samples: &[f32]) -> Result<(), WriteError> {
        let channels = usize::from(self.format.channels());
        if !samples.len().is_multiple_of(channels) {
            return Err(WriteError::IncompleteFrame);
        }
        if self.is_closed() {
            return Err(WriteError::Closed);
        }
        let available_frames = self.available_frames();
        if samples.len() / channels > available_frames {
            return Err(WriteError::Full { available_frames });
        }
        // This is the only sender, so capacity cannot decrease after the check.
        for &sample in samples {
            let sample = if sample.is_nan() {
                0.0
            } else {
                sample.clamp(-1.0, 1.0)
            };
            match self.sender.try_send(sample) {
                Ok(()) => {}
                Err(TrySendError::Closed(_)) => return Err(WriteError::Closed),
                Err(TrySendError::Full(_)) => unreachable!("unique producer checked enough space"),
            }
        }
        Ok(())
    }
}

/// Unique PCM consumer. Dropping it closes submission. Reads consume complete
/// frames and fill any missing tail with silence.
pub struct PcmReader {
    pub(crate) receiver: Receiver<f32>,
    format: Format,
}

impl PcmReader {
    pub fn format(&self) -> Format {
        self.format
    }

    /// Returns the number of source frames read, excluding inserted silence.
    pub fn read(&mut self, output: &mut [f32]) -> Result<usize, ConfigError> {
        self.read_mapped(output, |sample| sample)
    }

    pub(crate) fn read_mapped<T>(
        &mut self,
        output: &mut [T],
        mut convert: impl FnMut(f32) -> T,
    ) -> Result<usize, ConfigError> {
        let channels = usize::from(self.format.channels());
        if !output.len().is_multiple_of(channels) {
            return Err(ConfigError("output must contain complete frames"));
        }
        // Take a whole-frame snapshot: the producer may be midway through
        // sending a frame. Only this reader removes samples.
        let frames = self.receiver.len().min(output.len()) / channels;
        for (index, sample) in output.iter_mut().enumerate() {
            *sample = convert(if index < frames * channels {
                self.receiver
                    .try_recv()
                    .expect("observed samples remain available")
            } else {
                0.0
            });
        }
        Ok(frames)
    }
}

impl Drop for PcmReader {
    fn drop(&mut self) {
        self.receiver.close();
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WriteError {
    Closed,
    IncompleteFrame,
    Full { available_frames: usize },
}

impl fmt::Display for WriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Closed => f.write_str("audio output is closed"),
            Self::IncompleteFrame => f.write_str("samples must contain complete frames"),
            Self::Full { available_frames } => write!(
                f,
                "audio buffer is full ({available_frames} frames available)"
            ),
        }
    }
}
impl Error for WriteError {}

#[cfg(test)]
mod tests;
