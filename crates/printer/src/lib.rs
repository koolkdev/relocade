//! Printer peripherals consuming an explicitly connected parallel byte stream.
#![forbid(unsafe_code)]

mod capture;
pub use capture::Capture;

pub mod text;
