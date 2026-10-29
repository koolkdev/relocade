//! GPU images and surface-independent viewing on native and browser hosts.
//!
//! [`Image`] supports explicit CPU upload and GPU rendering into reusable RGBA8
//! storage. [`Viewer`] places an image into a caller-supplied output attachment.
//! The owner supplies the device and queue, orders updates and viewing, and
//! manages surfaces, workers and redraw scheduling.
#![forbid(unsafe_code)]

mod error;
mod image;
mod viewer;

pub use error::FrameError;
pub use image::Image;
pub use viewer::Viewer;
