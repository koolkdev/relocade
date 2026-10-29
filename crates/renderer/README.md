# GPU images and viewing

`Image` owns reusable RGBA8 GPU storage and its pixel aspect ratio. GPU producers
can draw into `image.view()`; the software path calls `image.upload(queue, pixels)`.
An upload takes tightly packed top-to-bottom sRGB color bytes and queues them for
the next queue submission. Image clones share storage, not historical pixels.

`Viewer` draws an existing image into a caller-supplied texture view. It uses
nearest-neighbor scaling, fits the image's display proportions inside the output,
and clears unused borders to opaque black. It supports RGBA8 and BGRA8 targets,
with or without sRGB encoding. Alpha is copied without blending. sRGB outputs
use a shader decode before the attachment re-encodes the colors.

```rust,ignore
let image = relocade_renderer::Image::new(&device, [frame.width, frame.height],
    frame.pixel_aspect_ratio)?;
image.upload(&queue, &frame.pixels)?;
let mut viewer = relocade_renderer::Viewer::new(device, queue, output_format)?;
viewer.draw(&image, &output_view, output_size)?;
// Later, reuse the image with another output size; no upload is needed.
```

The owner supplies a device, its queue and single-sampled output attachments.
The `output_size` is the extent of the supplied attachment's mip level. Surface
acquisition, presentation, visibility, worker placement and redraw scheduling
remain with that owner. Neither image upload nor viewing selects a surface or
creates a task. A different output format needs a matching viewer, but the same
image can be viewed through either format.

Dimensions describe pixel storage; pixel aspect ratio describes pixel width /
height. Their product determines display proportions. The VGA software image is
640×200 with 5:12 pixels, so the viewer fits it at 4:3. Output sizing never changes
the source image or asks its producer to decode it again.

`image.texture()` permits explicit copies and test readback. Ordinary drawing
keeps the image on the GPU. The native/browser pixel checks cover uploads, GPU
attachment writes, orientation, scaling, letterboxing, color encoding, alpha and
resizing. The [worker fixture](../../tests/browser/src/graphics.rs) demonstrates
an application-selected hosting arrangement and retains its own surface lifetime.
