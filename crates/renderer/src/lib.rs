//! Present RGBA pixel frames through wgpu on native and browser hosts.
//!
//! [`Renderer`] owns the frame texture and draw pipeline. The embedding supplies
//! a device, queue and output texture view, and owns surface configuration,
//! presentation and frame scheduling. There is no dependency on DOS or BIOS.
#![forbid(unsafe_code)]

mod frame;

pub use frame::{FrameError, Renderer};
