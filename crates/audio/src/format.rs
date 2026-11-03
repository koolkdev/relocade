use std::{error::Error, fmt};

/// Interleaved PCM format. Mono has one channel; stereo is left, then right.
/// Other layouts use the output device's channel order without remapping.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Format {
    sample_rate: u32,
    channels: u16,
}

impl Format {
    /// Describes samples at a nonzero rate with 1–32 channels.
    pub fn new(sample_rate: u32, channels: u16) -> Result<Self, ConfigError> {
        if sample_rate == 0 {
            return Err(ConfigError("sample rate must be nonzero"));
        }
        if !(1..=32).contains(&channels) {
            return Err(ConfigError("channel count must be between 1 and 32"));
        }
        Ok(Self {
            sample_rate,
            channels,
        })
    }

    pub fn sample_rate(self) -> u32 {
        self.sample_rate
    }
    pub fn channels(self) -> u16 {
        self.channels
    }
}

/// Invalid format, capacity, or incomplete interleaved frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConfigError(pub(crate) &'static str);

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl Error for ConfigError {}
