# Rendering and window presentation

A `Renderer` is a worker-local object that owns graphics state and its current
`Texture`. `Renderer::new(source, Graphics)` constructs it on the graphics worker;
only the source crosses workers. The renderer need not be `Send` or `Unpin`.
A source can contain application-specific drawing requests and reply handles.

The renderer implements `Future<Output = Result<(), String>>`. An embedding pins
and polls it alongside other work, independently of window redraws. Each poll
must be bounded, and native readbacks must drive device polling without relying
on window frames. Dropping the renderer cancels its pending work on that worker.
A completed renderer must not be polled again. The embedding can retain its final
texture before releasing the renderer's remaining resources.

`Renderer::texture()` borrows the current display texture, or returns `None`
until one is available. The renderer can update it or replace it when dimensions
change. This is live GPU storage: cloning a handle does not freeze a frame.
Updates and viewer draws are submitted in order through the shared device and
queue; no texture publication callback or cross-worker transfer is needed.

```rust,ignore
let mut renderer = Box::pin(MyRenderer::new(source, graphics)?);
// Poll renderer independently of window redraws.
// After acquiring a window frame, borrow the current texture:
if let Some(texture) = renderer.as_ref().get_ref().texture() {
    viewer.draw(texture, &host_target);
}
```

## GPU resources

`Texture` owns reusable GPU storage with its own dimensions. It can receive tightly
packed RGBA8 pixels through `write`, or serve as a render attachment through
`view`. Uploads submit their own work, so they progress without a host redraw.
Direct GPU rendering uses `view`; copies and readbacks use the wgpu texture
returned by `raw`. Both use the embedding's device and queue.

A texture stores sRGB colors in RGBA8 with unchanged alpha. Its render attachment
uses `Texture::FORMAT` (`Rgba8UnormSrgb`), so shader output is linear color. Clones
share a reusable texture on the same worker; they are not historical snapshots.
Submit texture updates before the viewer samples it. Queue order
keeps those updates ahead of host sampling without copying pixels back to the CPU.

`Viewer` draws a texture into a host texture view with nearest-neighbor scaling and
no blending. Host output supports RGBA8 or BGRA8, with sRGB or unorm storage. The
viewer chooses the matching sample view to preserve encoded colors and alpha.
It retains its pipeline and binding while the texture is reused. Replacing a texture
or changing the host dimensions does not require rebuilding the viewer.

```rust,ignore
let texture = relocade_renderer::Texture::new(&device, texture_size)?;
texture.write(&queue, rgba_pixels)?;

// Independently, when the host requests a frame:
let mut viewer = relocade_renderer::Viewer::new(device, queue, host_format)?;
viewer.draw(&texture, &host_target);
```

The embedding owns device and surface creation, host sizing, acquisition and
presentation. Texture dimensions and rendering cadence belong to the producer.
For a DOS display, the host can choose a 4:3 target even though the texture has
640×200 pixels.
Zero-sized host surfaces should not be acquired.

The [application crate](../app/README.md) reuses this rendering contract.
The optional [VGA renderer](../vga/README.md#gpu-renderer) consumes immutable
published screens independently of the live device.

The `pixels` integration test reads back actual native and browser GPU output to
check orientation, nearest-neighbor scaling, color encoding, alpha, updates,
source replacement and output resizing. It covers both pixel uploads and direct
GPU rendering. The VGA crate owns rasterized-screen upload and timer coverage.
