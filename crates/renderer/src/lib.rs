//! GPU image storage and drawing operations for caller-owned render passes.
//!
//! [`Image`] supports explicit CPU upload and GPU rendering into reusable RGBA8
//! storage. [`Renderer::draw_image`] records a draw at a caller-selected destination;
//! [`fit_image`] calculates aspect-preserving placement without GPU commands.
//! The caller owns command recording, output passes, clearing and submission,
//! along with surface, worker and redraw lifetimes.
#![forbid(unsafe_code)]

mod error;
mod geometry;
mod image;
mod renderer;

pub use error::FrameError;
pub use geometry::{fit_image, Rect};
pub use image::Image;
pub use renderer::Renderer;
