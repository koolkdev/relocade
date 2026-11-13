//! AdLib sound synthesis, independent of platform audio output.
#![forbid(unsafe_code)]

mod opl2;
pub use opl2::Opl2;
