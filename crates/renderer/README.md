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

VGA publishes ready-to-present pixel frames:

```rust,ignore
let display = vga.borrow().display(); // VGA is registered with the machine runner.
let mut renderer = relocade_renderer::Renderer::new(device, queue, format)?;

// On the renderer worker, retain the latest completed frame:
let frame = display.next_frame(None).await.unwrap();
// Acquire the surface texture and create its `view` here.
renderer.render(
    [frame.width, frame.height],
    &frame.pixels,
    &view,
)?;
// Present that texture here.
```

The execution worker services VGA deadlines, rasterizes text and publishes
immutable RGBA images through the display handle. The graphics worker only
uploads and presents those pixels. The browser main thread handles canvas events.
The renderer worker can own the device, queue, surface and presentation lifecycle.
The embedding supplies platform surface setup and forwards resize/lifecycle
messages. See the [VGA threading contract](../vga/README.md).

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
