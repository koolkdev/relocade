//! Shared VGA helpers and mode-specific decoding form one WGSL module.

// WGSL has no source includes; compose the files at compile time.
pub(super) const TEXT: &str = concat!(
    include_str!("shaders/resources.wgsl"),
    include_str!("shaders/addressing.wgsl"),
    include_str!("shaders/palette.wgsl"),
    include_str!("shaders/text.wgsl"),
);
