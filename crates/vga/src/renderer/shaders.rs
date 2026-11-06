//! Separate VGA and packed SVGA modules share only storage, DAC and drawing helpers.

// WGSL has no source includes; compose the files at compile time.
pub(super) const VGA: &str = concat!(
    include_str!("shaders/resources.wgsl"),
    include_str!("shaders/dac.wgsl"),
    include_str!("shaders/vga_state.wgsl"),
    include_str!("shaders/addressing.wgsl"),
    include_str!("shaders/serializer.wgsl"),
    include_str!("shaders/blanking.wgsl"),
    include_str!("shaders/attribute.wgsl"),
    include_str!("shaders/text.wgsl"),
    include_str!("shaders/indexed256.wgsl"),
    include_str!("shaders/graphics16.wgsl"),
    include_str!("shaders/vga.wgsl"),
);

pub(super) const SVGA: &str = concat!(
    include_str!("shaders/resources.wgsl"),
    include_str!("shaders/dac.wgsl"),
    include_str!("shaders/packed.wgsl"),
);
