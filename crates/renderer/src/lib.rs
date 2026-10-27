//! GPU textures and window presentation through wgpu on native and browser hosts.
//!
//! [`Renderer`] owns graphics work and exposes its current texture.
//! [`Texture`] owns a GPU texture independently of a host surface. [`Viewer`]
//! scales a texture into a host attachment. The embedding owns device creation,
//! surface configuration, presentation and the scheduling of both stages.
#![forbid(unsafe_code)]

mod error;
mod renderer;
mod texture;
mod viewer;

pub use error::Error;
pub use renderer::{Graphics, Renderer};
pub use texture::Texture;
pub use viewer::Viewer;
