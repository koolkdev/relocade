# GPU images and drawing

`Image` owns reusable RGBA8 GPU storage and its pixel aspect ratio. GPU producers
can draw into `image.view()`; CPU input uses `image.upload(queue, pixels)`. Uploads
contain tightly packed top-to-bottom sRGB color bytes and are queued for the
caller's next submission. Select one upload per image per submitted batch.
Image clones share storage rather than preserving historical pixels.

`Renderer::draw_image` draws the whole image into an explicit destination in a
caller-owned render pass. It uses nearest sampling and copies alpha without
blending. Supported outputs are RGBA8 and BGRA8, with or without sRGB encoding.
sRGB outputs use a shader decode before the attachment re-encodes the colors.
The image format is fixed RGBA8Unorm containing sRGB-encoded colors.

```rust,ignore
let mut renderer = relocade_renderer::Renderer::new(device.clone(), output_format)?;
let destination = relocade_renderer::fit_image(&image, available_rect);
let mut encoder = device.create_command_encoder(&Default::default());
{
    let mut pass = encoder.begin_render_pass(&output_pass);
    renderer.draw_image(&mut pass, &image, destination);
    renderer.draw_image(&mut pass, &other_image, other_destination);
}
queue.submit([encoder.finish()]);
```

The caller chooses the attachments, load/clear policy, placement and submission.
Drawing changes pipeline, binding 0 and viewport state, while retaining the
caller's scissor. The pass must use the supplied device, configured output format
and one sample. Destinations must fit inside the attachment; empty rectangles
record no draw. Surfaces, presentation, workers and redraw scheduling belong to
the embedding.

`Rect` describes output pixel coordinates. `fit_image` centers an image within
that rectangle using its dimensions and pixel aspect ratio, without GPU work.
The fixed VGA image is 640×200 with 5:12 pixels, so it fits at 4:3. Fitting is an
optional caller operation; `draw_image` uses exactly the destination it receives.
Changing placement or output size does not update the source image.

`image.texture()` permits explicit copies and test readback. Ordinary drawing
keeps pixels on the GPU. Native/browser pixel checks cover uploads, GPU attachment
writes, orientation, scaling, caller-selected placement, color encoding and alpha.
The [worker fixture](../../tests/browser/src/graphics.rs) owns its surface and
command submission independently of the image-drawing library.
