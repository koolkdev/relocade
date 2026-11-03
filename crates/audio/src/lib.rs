//! Bounded PCM transport for application-owned audio producers and outputs.
//!
//! The application owns synthesis and scheduling. [`pcm_buffer`] connects a
//! single producer to a consumer with channel try operations. Frames contain
//! interleaved floating-point samples in the negotiated [`Format`].
#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

mod format;
mod output;
mod pcm;

pub use format::{ConfigError, Format};
pub use output::{Output, OutputError};
pub use pcm::{pcm_buffer, PcmReader, PcmWriter, WriteError};

#[cfg(all(test, target_arch = "wasm32"))]
#[path = "tests/browser.rs"]
mod browser_tests;
