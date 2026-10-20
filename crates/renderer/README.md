# Pixel-frame rendering

`Renderer` accepts a frame's dimensions and tightly packed RGBA8 bytes, then draws
into a wgpu texture view. The bytes contain sRGB colors; output can be RGBA8 or
BGRA8 with either linear storage or an sRGB attachment. Alpha is copied without
blending. Nearest-neighbor sampling fills the output view.

The embedding creates the wgpu device and surface, configures the output size,
acquires/presents surface textures and schedules frames. The renderer reuses its
frame texture while the source size is unchanged. Changing source or output
dimensions needs no renderer reconstruction, provided the output format stays
the same. Skip surface acquisition while the window/canvas has zero size.

For the current text screen, the two crates connect directly:

```rust,ignore
let mut text = relocade_bios::TextRenderer::new();
let mut renderer = relocade_renderer::Renderer::new(device, queue, format)?;

// On each redraw, after acquiring an output texture view:
let screen = relocade_bios::TextScreen::read(&memory);
let pixels = text.render(&screen, elapsed);
renderer.render(
    [relocade_bios::TextRenderer::WIDTH, relocade_bios::TextRenderer::HEIGHT],
    pixels,
    &view,
)?;
```

No emulation state crosses into the GPU renderer. A native window and browser
canvas use the same path. Aspect ratio belongs to the embedding's output sizing;
for a DOS display it can choose a 4:3 target even though the source has 640×200 pixels.

The `pixels` integration test reads back actual GPU output to check orientation,
nearest-neighbor scaling, color encoding, alpha, updates and resizing. It runs on
native wgpu and browser WebGPU. The native variant also checks BIOS text output.
