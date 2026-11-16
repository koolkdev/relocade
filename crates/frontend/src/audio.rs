//! Shared audio timing and device-owned source registration.

mod sources;
mod timing;

pub use sources::Sources;
pub use timing::AudioTiming;

/// Interleaved PCM format and preferred frames per calculation block.
#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub sample_rate: u32,
    pub channels: u16,
    pub block_frames: usize,
}
