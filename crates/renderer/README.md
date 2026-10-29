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

VGA supplies immutable captured state independently of this pixel presenter.
The embedding can call `CapturedFrame::rasterize()` when it wants a software
image, then draw the result:

```rust,ignore
let frame = capture.rasterize();
renderer.render([frame.width, frame.height], &frame.pixels, &view)?;
```

The [graphics fixture](../../tests/browser/src/graphics.rs) demonstrates doing
conversion and presentation on one worker separate from CPU/VGA execution.
The application owns that worker, its request channel, cached pixels and surface.
Other renderers can read the captured text state directly without producing
CPU pixels or using this presenter. See the [VGA contract](../vga/README.md).

No emulation state crosses into the GPU renderer. A native window and browser
canvas use the same pixel-rendering path. Aspect ratio belongs to the embedding's
output sizing; for a DOS display it can choose a 4:3 target even though the source
has 640×200 pixels. Its display ratio is
`width * pixel_aspect_ratio.0 / (height * pixel_aspect_ratio.1)`; the generic
renderer does not interpret that metadata.

The `pixels` integration test reads back actual GPU output to check orientation,
nearest-neighbor scaling, color encoding, alpha, updates and resizing. It runs on
native wgpu and browser WebGPU. The native variant also checks a rasterized VGA
text frame.
